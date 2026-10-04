"""Splits keywords_raw.txt into the keyword model's word pieces (needs `pip install sentencepiece`)."""
import sentencepiece as spm

sp = spm.SentencePieceProcessor(model_file="models/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/bpe.model")
lines = []
for phrase in open("keywords_raw.txt").read().split("\n"):
    if phrase.strip():
        lines.append(" ".join(sp.encode(phrase.upper(), out_type=str)) + " @" + phrase.replace(" ", "_"))
open("keywords.txt", "w").write("\n".join(lines) + "\n")
