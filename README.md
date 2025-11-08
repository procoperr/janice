# Janice `jan`

A file sync tool that refuses to waste your time.

## Why

Because copying the same 5GB file *again* just because you renamed it is barbaric.

## What it does

It looks at both sides, figures out what's actually different, and only touches that. No drama, no wasted bandwidth, no "oops I recopied a 12GB video because I renamed a folder" energy. BLAKE3 keeps hashing fast, streaming keeps memory flat, and rename detection keeps your mistakes cheap.

## Install

```bash
cargo install janice
```

Or grab a binary if compiling triggers your fight‑or‑flight.

## Use

```bash
jan SOURCE DEST
```

Useful flags:

```bash
-n  dry run (trust issues)
-d  delete files in DEST that aren't in SOURCE
-y  don't ask questions
-j N  threads (more threads, more fan noise)
-q  silence
-v  the opposite of silence
```

Example:

```bash
jan ~/stuff /mnt/backup/stuff -qdy
```

Runs nightly, never speaks, never complains. A role model. Jealous yet?

## How it works

Hashes everything, compares fingerprints, moves what's moved, copies what's new, ignores what’s unchanged. All while pretending not to care.

## Planned

* [ ] **Robust delta engine** (rsync-style rolling + content-defined chunking)
* [ ] **Native SSH transport** (no more mount workarounds)
* [ ] **Resumable transfers** (interruption is not failure)
* [ ] **Atomic apply** (no half-synced nightmare states)
* [ ] **Merkle/BLAKE3 verification** (trust, but *actually* verify)

## License

MIT. Do whatever. Couldn't care. Godspeed.
