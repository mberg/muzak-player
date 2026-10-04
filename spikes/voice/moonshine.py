"""Transcribes the test clips with Moonshine and prints "clip<TAB>transcript" lines for `score`.

Usage: python moonshine.py <tiny_streaming|small_streaming|medium_streaming|tiny|base> [keyterms] > out.tsv
Timing and peak memory go to stderr.
"""
import glob, resource, sys, time

import moonshine_voice as mv
from moonshine_voice.transcriber import Transcriber

arch = getattr(mv.ModelArch, sys.argv[1].upper())
keyterms = len(sys.argv) > 2 and sys.argv[2] == "keyterms"
path, arch = mv.get_model_for_language("en", arch, on_progress=lambda *_: None)
t = Transcriber(path, arch, options={"num_threads": "1"} if False else None)
if keyterms:
    t.set_keyterms(["Graceland", "The Hobbit", "Paul Simon", "Matilda", "Road Trip"])
total_audio = busy = 0.0
for wav in sorted(glob.glob("wavs/*.wav")):
    audio, rate = mv.load_wav_file(wav)
    total_audio += len(audio) / rate
    start = time.perf_counter()
    transcript = t.transcribe_without_streaming(audio, rate)
    busy += time.perf_counter() - start
    text = " ".join(line.text for line in transcript.lines).strip()
    print(f"{wav}\t{text}", flush=True)
peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
peak_mb = peak / 1e6 if sys.platform == "darwin" else peak / 1e3
print(
    f"{sys.argv[1]}{' + keyterms' if keyterms else ''}: {total_audio:.1f} s of audio in {busy:.2f} s "
    f"({100 * busy / total_audio:.1f}% of real time); peak memory {peak_mb:.0f} MB",
    file=sys.stderr,
)
