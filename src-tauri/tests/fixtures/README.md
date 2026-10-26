# Test fixtures

Synthetic speech generated with espeak-ng (no human recordings), converted to
16 kHz mono 16-bit PCM with ffmpeg:

| File | Command |
| --- | --- |
| `quick_brown_fox_en.wav` | `espeak-ng -v en-gb-x-rp -s 130 "The quick brown fox jumps over the lazy dog. Please send the report by Friday."` |
| `guten_morgen_de.wav` | `espeak-ng -v de -s 140 "Guten Morgen, wie geht es dir heute?"` |
