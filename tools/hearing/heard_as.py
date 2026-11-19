"""heard_as: what an audio-text model hears in a WAV (a hint, not a check).

A worker for sounder's `listen`: it loads the model once, then answers one
JSON request per stdin line, one JSON reply per stdout line:

  {"wav": "/abs/x.wav", "brief": "a wooden crate dropped on concrete"}
  -> {"top": [["a wooden box dropped on the floor", 0.41], ...],
      "brief": {"score": 0.38, "rank": 3, "of": 106}}

`brief` is optional; its rank is among the vocabulary plus the brief.
Run with the Python of a venv that has torch, transformers and soundfile:
  python -I tools/hearing/heard_as.py [--model laion/larger_clap_general]
On 40 labelled cues this model named the intended family first on 18 and
ranked good above bad cues with AUC 0.80 (docs/STATUS.md).
"""
import json
import os
import sys

MODEL = "laion/larger_clap_general"
TOP = 5


def main():
    args = sys.argv[1:]
    name = args[args.index("--model") + 1] if "--model" in args else MODEL
    vocab_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "vocabulary.txt")
    vocab = [l.strip() for l in open(vocab_path) if l.strip() and not l.startswith("#")]
    import numpy as np
    import soundfile as sf
    import torch
    from transformers import ClapModel, ClapProcessor

    torch.set_num_threads(max(1, (os.cpu_count() or 2) // 2))
    model = ClapModel.from_pretrained(name).eval()
    proc = ClapProcessor.from_pretrained(name)
    sr_model = proc.feature_extractor.sampling_rate

    def feats(o):
        o = o if isinstance(o, torch.Tensor) else o.pooler_output
        return torch.nn.functional.normalize(o, dim=-1)

    def text(ts):
        with torch.no_grad():
            return feats(model.get_text_features(**proc(text=["the sound of " + t for t in ts], return_tensors="pt", padding=True)))

    V = text(vocab)
    print(json.dumps({"ready": name, "vocabulary": len(vocab)}), flush=True)
    for line in sys.stdin:
        try:
            req = json.loads(line)
            x, sr = sf.read(req["wav"], dtype="float32")
            x = x.mean(1) if x.ndim > 1 else x
            if sr != sr_model:
                n = int(round(len(x) * sr_model / sr))
                x = np.interp(np.linspace(0, len(x) - 1, n), np.arange(len(x)), x).astype("float32")
            x = x / (np.abs(x).max() + 1e-9) * 0.5
            with torch.no_grad():
                A = feats(model.get_audio_features(**proc(audio=x, sampling_rate=sr_model, return_tensors="pt")))
            s = (A @ V.T)[0].numpy()
            order = np.argsort(-s)[:TOP]
            out = {"top": [[vocab[i], round(float(s[i]), 3)] for i in order]}
            brief = (req.get("brief") or "").strip()
            if brief:
                b = float((A @ text([brief]).T)[0, 0])
                out["brief"] = {"score": round(b, 3), "rank": 1 + int((s > b).sum()), "of": len(vocab) + 1}
            print(json.dumps(out), flush=True)
        except Exception as e:  # one bad request must not end the worker
            print(json.dumps({"error": f"{type(e).__name__}: {e}"}), flush=True)


if __name__ == "__main__":
    main()
