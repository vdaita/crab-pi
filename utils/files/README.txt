Copy these files to the root of your Pi FAT32 partition:
- GPTW.BIN
- GPTTOK.TXT

Optional metadata/debug files:
- GPTLAYRS.JSON
- GPTMETA.JSON

Generation flow:
1) From utils/, run: uv run convert_weights.py
2) Copy output files from utils/files/ onto SD root.
3) Boot crab-pi and run gpt::gpt_demo().

Notes:
- Loader prefers exact names GPTW.BIN and GPTTOK.TXT.
- If exact names are absent, loader falls back to existing .BIN/.TXT heuristics.
