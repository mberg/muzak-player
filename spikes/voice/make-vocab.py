"""Writes bpe.vocab next to the transcription model, for HOTWORDS (needs `pip install sentencepiece`)."""
import sentencepiece as spm

model = "models/sherpa-onnx-streaming-zipformer-en-2023-06-26"
sp = spm.SentencePieceProcessor(model_file=f"{model}/bpe.model")
with open(f"{model}/bpe.vocab", "w") as out:
    for i in range(sp.get_piece_size()):
        out.write(f"{sp.id_to_piece(i)}\t{sp.get_score(i)}\n")
