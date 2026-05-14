import typer
import torch
from datasets import load_dataset

from train_tiny_model import CharGPT, DEVICE, SAVE_PATH


def build_vocab(target_chars: int = 10_000_000):
    ds = load_dataset("roneneldan/TinyStories", split="train", streaming=True)
    texts, total = [], 0
    for ex in ds:
        t = ex["text"]
        texts.append(t)
        total += len(t)
        if total >= target_chars:
            break
    text = "\n".join(texts)
    chars = sorted(set(text))
    stoi = {c: i for i, c in enumerate(chars)}
    itos = {i: c for i, c in enumerate(chars)}
    return stoi, itos, len(chars)


@torch.no_grad()
def sample_text(model: CharGPT, stoi, itos, prompt: str, max_new_tokens: int, temperature: float, top_k: int):
    idx = torch.tensor([[stoi[c] for c in prompt if c in stoi]], dtype=torch.long, device=DEVICE)
    if idx.numel() == 0:
        idx = torch.zeros((1, 1), dtype=torch.long, device=DEVICE)
    out = model.generate(idx, max_new_tokens=max_new_tokens, temperature=temperature, top_k=top_k)
    return "".join(itos[i.item()] for i in out[0])


def main(
    prompt: str = typer.Option("Once upon a time", "--prompt"),
    samples: int = typer.Option(3, "--samples", min=1),
    max_new_tokens: int = typer.Option(300, "--max-new-tokens", min=1),
    temperature: float = typer.Option(0.8, "--temperature", min=0.05),
    top_k: int = typer.Option(40, "--top-k", min=0),
):
    stoi, itos, vocab_size = build_vocab()
    model = CharGPT(vocab_size).to(DEVICE)
    model.load_state_dict(torch.load(SAVE_PATH, map_location=DEVICE))
    model.eval()
    print(f"device={DEVICE} vocab={vocab_size} samples={samples}")
    for i in range(samples):
        print(f"\n=== Sample {i + 1} ===")
        print(sample_text(model, stoi, itos, prompt, max_new_tokens, temperature, top_k))


if __name__ == "__main__":
    typer.run(main)
