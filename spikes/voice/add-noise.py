"""Pads a clip with silence and writes a clean copy and one with background noise."""
import math, random, struct, sys, wave

src, f, expect, voice, text = sys.argv[1:]
w = wave.open(src)
n = w.getnframes()
data = struct.unpack(f"<{n}h", w.readframes(n))
pad = [0] * 8000
clean = pad + list(data) + pad * 2


def save(name, samples):
    o = wave.open(name, "w")
    o.setnchannels(1)
    o.setsampwidth(2)
    o.setframerate(16000)
    o.writeframes(struct.pack(f"<{len(samples)}h", *[max(-32768, min(32767, int(s))) for s in samples]))
    o.close()


tag = f"{f}__{expect}__{voice}"
save(f"{tag}__clean.wav", clean)
# Rumbly noise about 10 dB below the speech, roughly like music in the room.
rms = math.sqrt(sum(s * s for s in data) / max(1, len(data)))
random.seed(int(f))
b = 0.0
noisy = []
for s in clean:
    b = 0.97 * b + random.gauss(0, 1)
    noisy.append(s + b * rms * 0.316 / 5.8)
save(f"{tag}__noise.wav", noisy)
open(f"{tag}.txt", "w").write(text)
