import json
import os
import sys
import numpy as np
from huggingface_hub import HfApi, hf_hub_download

REPO = "calum/tinystories-gpt2-3M"
PREFIX = "tinygpt"
OUT_DIR = os.path.join(os.path.dirname(__file__), "files")

WEIGHTS_OUT = "GPTW.BIN"
TOKENIZER_OUT = "GPTTOK.TXT"
LAYERS_OUT = "GPTLAYRS.JSON"
META_OUT = "GPTMETA.JSON"

if __name__ == "__main__":
	os.makedirs(OUT_DIR, exist_ok=True)

	files = HfApi().list_repo_files(REPO)
	model_candidates = []
	for f in files:
		fl = f.lower()
		if not (fl.endswith(".safetensors") or fl.endswith(".pt") or fl.endswith(".bin") or fl.endswith(".pth")):
			continue
		if "optimizer" in fl or "sched" in fl or "trainer" in fl:
			continue
		model_candidates.append(f)

	model = None
	for suffix in (".safetensors", ".bin", ".pth", ".pt"):
		preferred = next((f for f in model_candidates if f.lower().endswith(suffix) and ("model" in f.lower() or "pytorch" in f.lower() or "checkpoint" in f.lower())), None)
		if preferred:
			model = preferred
			break
	if model is None:
		for suffix in (".safetensors", ".bin", ".pth", ".pt"):
			fallback = next((f for f in model_candidates if f.lower().endswith(suffix)), None)
			if fallback:
				model = fallback
				break
	if model is None:
		print("no model file found in repo", REPO)
		sys.exit(1)

	model_path = hf_hub_download(REPO, filename=model)
	sd = {}
	if model_path.endswith(".safetensors"):
		try:
			from safetensors.numpy import load_file

			sd = load_file(model_path)
		except Exception:
			try:
				import torch

				obj = torch.load(model_path, map_location="cpu")
				sd = obj.get("state_dict", obj) if isinstance(obj, dict) else {}
			except Exception as e:
				print("failed to load model:", e)
				sys.exit(1)
	else:
		import torch

		obj = torch.load(model_path, map_location="cpu")
		sd = obj.get("state_dict", obj) if isinstance(obj, dict) else {}

	if not isinstance(sd, dict):
		print("loaded weights are not a dict")
		sys.exit(1)

	# write weights and layers
	layers = {}
	weights_out_path = os.path.join(OUT_DIR, WEIGHTS_OUT)
	skipped = []
	with open(weights_out_path, "wb") as bf:
		for name in sorted(sd.keys()):
			val = sd[name]
			if isinstance(val, dict):
				skipped.append(name)
				continue
			if hasattr(val, "detach"):
				arr = val.detach().float().cpu().numpy()
			else:
				try:
					arr = np.asarray(val)
				except Exception:
					skipped.append(name)
					continue
			if not np.issubdtype(arr.dtype, np.number):
				skipped.append(name)
				continue
			layers[name] = list(arr.shape)
			bf.write(arr.astype("float32").ravel().tobytes())
	layers_out_path = os.path.join(OUT_DIR, LAYERS_OUT)
	with open(layers_out_path, "w", encoding="utf-8") as jf:
		json.dump(layers, jf, indent=2)

	# tokenizer
	tok = next((f for f in files if f.lower().endswith("tokenizer.json")), None)
	if tok is None:
		tok = next((f for f in files if f.lower().endswith("vocab.json")), None)
	if tok is None:
		tok = next((f for f in files if "tokenizer" in f.lower() and f.lower().endswith(".json")), None)
	if tok is None:
		tok = next((f for f in files if "token" in f.lower() and f.lower().endswith(".json")), None)
	tokens = []
	if tok:
		tok_path = hf_hub_download(REPO, filename=tok)
		if tok_path.endswith(".json"):
			try:
				with open(tok_path, "r", encoding="utf-8") as tf:
					data = json.load(tf)
				if "model" in data and "vocab" in data["model"]:
					tokens = [k for k, _ in sorted(data["model"]["vocab"].items(), key=lambda kv: kv[1])]
				elif "vocab" in data:
					tokens = [k for k, _ in sorted(data["vocab"].items(), key=lambda kv: kv[1])]
			except Exception:
				tokens = []
		else:
			try:
				with open(tok_path, "r", encoding="utf-8") as tf:
					tokens = [l.rstrip("\n") for l in tf if l.strip()]
			except Exception:
				tokens = []

	if tokens:
		tok_out_path = os.path.join(OUT_DIR, TOKENIZER_OUT)
		with open(tok_out_path, "w", encoding="utf-8") as ttf:
			for t in tokens:
				ttf.write(t + "\n")

	meta_out_path = os.path.join(OUT_DIR, META_OUT)
	with open(meta_out_path, "w", encoding="utf-8") as mf:
		json.dump(
			{
				"repo": REPO,
				"original_prefix": PREFIX,
				"model_source_file": model,
				"weights_file": WEIGHTS_OUT,
				"tokenizer_file": TOKENIZER_OUT,
				"layers_file": LAYERS_OUT,
				"token_count": len(tokens),
			},
			mf,
			indent=2,
		)

	print("wrote SD bundle to", OUT_DIR)
	print(" -", WEIGHTS_OUT)
	print(" -", TOKENIZER_OUT)
	print(" -", LAYERS_OUT)
	print(" -", META_OUT)
	if skipped:
		print("skipped non-numeric entries:", len(skipped))

