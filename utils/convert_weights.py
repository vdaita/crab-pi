

import json
from pathlib import Path

import torch
import typer

from train_tiny_model import N_HEAD, N_LAYER, SAVE_PATH


def as_f32(t: torch.Tensor) -> torch.Tensor:
    return t.detach().cpu().to(torch.float32).contiguous()


def write_bin(path: Path, t: torch.Tensor) -> None:
    path.write_bytes(as_f32(t).numpy().astype("<f4", copy=False).tobytes(order="C"))


def add_tensor(files, name, tensor, transpose=False):
    t = tensor.T if transpose else tensor
    files.append({"name": name, "tensor": t, "shape": list(t.shape), "elements": int(t.numel())})


def collect_named_tensors(state):
    files = []
    n_embd = state["tok_emb.weight"].shape[1]
    add_tensor(files, "TOK_EMB", state["tok_emb.weight"])
    add_tensor(files, "POS_EMB", state["pos_emb.weight"])
    for i in range(N_LAYER):
        p = f"blocks.{i}"
        add_tensor(files, f"L{i:02d}_LN1_W", state[f"{p}.ln1.weight"])
        add_tensor(files, f"L{i:02d}_LN1_B", state[f"{p}.ln1.bias"])
        c_attn = state[f"{p}.attn.c_attn.weight"]
        q, k, v = c_attn[:n_embd], c_attn[n_embd:2 * n_embd], c_attn[2 * n_embd:]
        add_tensor(files, f"L{i:02d}_ATTN_Q_W", q, transpose=True)
        add_tensor(files, f"L{i:02d}_ATTN_K_W", k, transpose=True)
        add_tensor(files, f"L{i:02d}_ATTN_V_W", v, transpose=True)
        add_tensor(files, f"L{i:02d}_ATTN_O_W", state[f"{p}.attn.c_proj.weight"], transpose=True)
        add_tensor(files, f"L{i:02d}_LN2_W", state[f"{p}.ln2.weight"])
        add_tensor(files, f"L{i:02d}_LN2_B", state[f"{p}.ln2.bias"])
        add_tensor(files, f"L{i:02d}_MLP_FC_W", state[f"{p}.mlp.fc1.weight"], transpose=True)
        add_tensor(files, f"L{i:02d}_MLP_PROJ_W", state[f"{p}.mlp.fc2.weight"], transpose=True)
    add_tensor(files, "LN_F_W", state["ln_f.weight"])
    add_tensor(files, "LN_F_B", state["ln_f.bias"])
    add_tensor(files, "LM_HEAD_W", state["head.weight"], transpose=True)
    return files


def write_outputs(files, out_dir: Path):
    out_dir.mkdir(parents=True, exist_ok=True)
    index = []
    packed = []
    offset = 0
    for entry in files:
        name, tensor = entry["name"], entry["tensor"]
        fname = f"{name}.BIN"
        write_bin(out_dir / fname, tensor)
        elems = entry["elements"]
        index.append({"name": name, "file": fname, "shape": entry["shape"], "offset": offset, "elements": elems})
        packed.append(tensor.reshape(-1))
        offset += elems
        print(f"wrote {fname:20s} shape={tuple(entry['shape'])} elems={elems}")
    flat = torch.cat([as_f32(t) for t in packed], dim=0)
    write_bin(out_dir / "GPTW.BIN", flat)
    (out_dir / "GPTLAYRS.JSON").write_text(json.dumps(index, indent=2))
    meta = {
        "n_layer": N_LAYER,
        "n_head": N_HEAD,
        "n_embd": files[0]["shape"][1],
        "vocab_size": files[0]["shape"][0],
        "n_ctx": files[1]["shape"][0],
        "dtype": "f32_le",
        "weights_file": "GPTW.BIN",
        "layer_files": [x["file"] for x in index],
    }
    (out_dir / "GPTMETA.JSON").write_text(json.dumps(meta, indent=2))
    print(f"wrote GPTW.BIN with {flat.numel()} floats")
    print(f"wrote {out_dir / 'GPTLAYRS.JSON'}")
    print(f"wrote {out_dir / 'GPTMETA.JSON'}")


def main(
    checkpoint: str = typer.Option(SAVE_PATH, "--checkpoint"),
    out_dir: str = typer.Option("files", "--out-dir"),
):
    ckpt_path = Path(checkpoint)
    if not ckpt_path.exists():
        raise typer.BadParameter(f"checkpoint not found: {ckpt_path}")
    state = torch.load(ckpt_path, map_location="cpu")
    files = collect_named_tensors(state)
    write_outputs(files, Path(out_dir))


if __name__ == "__main__":
    typer.run(main)

