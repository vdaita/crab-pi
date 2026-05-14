import math

import torch
import torch.nn as nn
import typer
from torch.nn import functional as F
from tqdm.auto import tqdm
from datasets import load_dataset

BATCH_SIZE, BLOCK_SIZE, MAX_ITERS, EVAL_EVERY, LR = 64, 384, 5000, 500, 3e-4
DEVICE = "mps" if torch.mps.is_available() else ("cuda" if torch.cuda.is_available() else "cpu")
N_LAYER, N_HEAD, N_EMBD, DROPOUT = 2, 4, 96, 0.1
SAVE_PATH = "char_model.pt"
app = typer.Typer(add_completion=False)

def load_data(target_chars=10_000_000):
    ds, texts, total = load_dataset("roneneldan/TinyStories", split="train", streaming=True), [], 0
    bar = tqdm(total=target_chars, desc="Loading", unit="char")
    for ex in ds:
        t = ex["text"]
        texts.append(t)
        total += len(t)
        bar.update(len(t))
        if total >= target_chars: break
    bar.close()
    text = "\n".join(texts)
    chars = sorted(set(text))
    stoi = {c: i for i, c in enumerate(chars)}
    data = torch.tensor([stoi[c] for c in text], dtype=torch.long)
    n = int(0.9 * len(data))
    return data[:n], data[n:], len(chars)
    
def get_batch(data):
    ix = torch.randint(len(data) - BLOCK_SIZE, (BATCH_SIZE,))
    x = torch.stack([data[i:i + BLOCK_SIZE] for i in ix])
    y = torch.stack([data[i + 1:i + BLOCK_SIZE + 1] for i in ix])
    return x.to(DEVICE), y.to(DEVICE)

class CausalSelfAttention(nn.Module):
    def __init__(self):
        super().__init__()
        self.c_attn, self.c_proj, self.drop = nn.Linear(N_EMBD, 3 * N_EMBD, bias=False), nn.Linear(N_EMBD, N_EMBD, bias=False), nn.Dropout(DROPOUT)
        self.register_buffer("bias", torch.tril(torch.ones(BLOCK_SIZE, BLOCK_SIZE)).view(1, 1, BLOCK_SIZE, BLOCK_SIZE))
    def forward(self, x):
        b, t, c = x.size(); q, k, v = self.c_attn(x).split(N_EMBD, dim=2)
        k = k.view(b, t, N_HEAD, c // N_HEAD).transpose(1, 2); q = q.view(b, t, N_HEAD, c // N_HEAD).transpose(1, 2); v = v.view(b, t, N_HEAD, c // N_HEAD).transpose(1, 2)
        att = (q @ k.transpose(-2, -1)) * (1.0 / math.sqrt(k.size(-1)))
        att = self.drop(F.softmax(att.masked_fill(self.bias[:, :, :t, :t] == 0, float("-inf")), dim=-1))
        return self.c_proj((att @ v).transpose(1, 2).contiguous().view(b, t, c))

class MLP(nn.Module):
    def __init__(self):
        super().__init__()
        self.fc1, self.fc2, self.drop = nn.Linear(N_EMBD, 4 * N_EMBD, bias=False), nn.Linear(4 * N_EMBD, N_EMBD, bias=False), nn.Dropout(DROPOUT)
    def forward(self, x): return self.drop(self.fc2(F.gelu(self.fc1(x))))

class Block(nn.Module):
    def __init__(self):
        super().__init__()
        self.ln1, self.ln2, self.attn, self.mlp = nn.LayerNorm(N_EMBD), nn.LayerNorm(N_EMBD), CausalSelfAttention(), MLP()
    def forward(self, x): return x + self.mlp(self.ln2(x + self.attn(self.ln1(x))))

class CharGPT(nn.Module):
    def __init__(self, vocab_size):
        super().__init__()
        self.tok_emb, self.pos_emb, self.drop = nn.Embedding(vocab_size, N_EMBD), nn.Embedding(BLOCK_SIZE, N_EMBD), nn.Dropout(DROPOUT)
        self.blocks, self.ln_f, self.head = nn.Sequential(*[Block() for _ in range(N_LAYER)]), nn.LayerNorm(N_EMBD), nn.Linear(N_EMBD, vocab_size, bias=False)
        print(f"Model parameters: {sum(p.numel() for p in self.parameters()):,}")
    def forward(self, idx, targets=None):
        b, t = idx.size(); x = self.ln_f(self.blocks(self.drop(self.tok_emb(idx) + self.pos_emb(torch.arange(t, device=idx.device))))); logits = self.head(x)
        return logits, (F.cross_entropy(logits.view(-1, logits.size(-1)), targets.view(-1)) if targets is not None else None)
    @torch.no_grad()
    def generate(self, idx, max_new_tokens=300, temperature=0.8, top_k=40):
        for _ in range(max_new_tokens):
            logits = self(idx[:, -BLOCK_SIZE:])[0][:, -1, :] / temperature
            if top_k: logits[logits < torch.topk(logits, min(top_k, logits.size(-1)))[0][:, [-1]]] = float("-inf")
            idx = torch.cat([idx, torch.multinomial(F.softmax(logits, dim=-1), 1)], dim=1)
        return idx

def train_model():
    train_data, val_data, vocab_size = load_data()
    model = CharGPT(vocab_size).to(DEVICE)
    opt = torch.optim.AdamW(model.parameters(), lr=LR)
    bar = tqdm(range(MAX_ITERS), desc="Training")
    for step in bar:
        model.train(); x, y = get_batch(train_data); _, loss = model(x, y)
        opt.zero_grad(); loss.backward(); torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0); opt.step()
        if step % EVAL_EVERY == 0:
            model.eval()
            with torch.no_grad(): _, vloss = model(*get_batch(val_data))
            bar.set_postfix(train=f"{loss.item():.4f}", val=f"{vloss.item():.4f}")
    torch.save(model.state_dict(), SAVE_PATH)

@app.command()
def main():
    train_model()
if __name__ == "__main__":
    app()