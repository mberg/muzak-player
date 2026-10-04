#!/bin/sh
# Makes test clips on a Mac with its built-in voices: each request, clean and with noise.
# Clips are named NNN__<expected command or none>__<voice>__<clean|noise>.wav.
# To test real voices instead, record 16 kHz mono WAVs with the same names into ./wavs.
set -e
mkdir -p wavs && cd wavs
i=0
while IFS='|' read -r expect text; do
  for v in Samantha Daniel Karen Fred; do
    i=$((i+1)); f=$(printf '%03d' $i)
    say -v $v -o raw.wav --data-format=LEI16@16000 "$text" || continue
    python3 ../add-noise.py raw.wav "$f" "$expect" "$v" "$text"
  done
done <<'LIST'
next_song|next song
pause|pause
pause|pause the music
turn_it_up|turn it up
play_graceland|play graceland
play_the_hobbit|play the hobbit
play_matilda|play matilda
play_road_trip|play road trip
go_back|go back
skip|skip
none|play paul simon's first album
none|what's the weather like today
none|can you play something by bon jovi
none|i'm going to the store later
none|play the album graceland by paul simon
LIST
rm -f raw.wav
ls *.wav | wc -l
