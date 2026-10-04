#!/bin/sh
# Downloads the two sherpa-onnx models the test uses into ./models.
set -e
mkdir -p models && cd models
base=https://github.com/k2-fsa/sherpa-onnx/releases/download
for m in kws-models/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01 \
         asr-models/sherpa-onnx-streaming-zipformer-en-2023-06-26; do
  name=${m#*/}
  [ -d "$name" ] || curl -sSL "$base/$m.tar.bz2" | tar xj
done
ls
