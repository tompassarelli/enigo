# X11 keyboard delay and shifted text repair

Base: Enigo 0.6.1, git b297a14e807abdf818b7809a70b445ade4fc5897 from
https://github.com/enigo-rs/enigo. The repository's MIT license and copyright
notice are preserved. No Cargo registry, consumer manifest, backend feature or
held Unicode Press/Release contract was changed.

The X11 connection now passes its configured delay into the keymap, and its
public getter/setter use that same state. Each emitted raw key operation
calculates its own delay, so mapping lookup cannot leave a stale delay for later
keys. Zero means zero; nonzero repeated-key delay still subtracts elapsed time,
with the existing one-millisecond spacing for distinct keys.

Unicode clicks (including text calls) use an existing level-one mapping with
Shift when level zero does not supply the requested symbol. Shift is acquired
only if it is not already down, and only that acquired Shift is released,
including after a failed click. Explicit Unicode Press/Release keeps its
existing layer-independent mapping behavior. Other modifier/layout semantics
are not redesigned.

## Observed check

The unchanged 24-packet, 290-byte counterexample from
wc3-melee:docs/enigo-text-counterexample-20261005 ran on a fresh Xvfb and xterm.
The exact same packet strings and public `Enigo::text` call were used with
`linux_delay=0`. All 290 receiver bytes matched, including the final newline.

| Host-monotonic text-call duration | Before | Repaired |
| --- | ---: | ---: |
| Median, 24 calls | 108.835 ms | 1.031 ms |
| Maximum | 518.153 ms | 2.755 ms |
| Minimum | 18.243 ms | 0.715 ms |

Five focused keymap regression tests pass: zero delay, nonzero elapsed spacing,
no stale delay on the following distinct key, constructor/setter consistency,
and existing shifted letter/punctuation lookup. A separate opt-in X11 test
passes with another connection holding Shift: the click leaves that Shift held
and adds no remapped keycode. The scratch X server and receiver exited normally;
capacity wrappers reported RELEASED. The compile reports one unused
`ModifierBitflag` alias warning in enigo:src/keycodes.rs.

This proves this X11 text boundary and byte corpus. Wine/Warcraft delivery and
native map admission still require the consumer's test. In particular, xterm
fidelity alone did not prove the prior Wine editbox path.

## Reproduce

Use Rust 1.96.1 and the existing Linux build dependencies. The shell script
allocates a new X display with `-displayfd`, runs the held-Shift test there, then
captures exact terminal bytes. It never targets a retained display. Outputs are
in enigo:target/repair-evidence; the observed packets, timings and received bytes
are retained alongside this record.

```sh
cd ~/code/enigo/worktrees/enigo-delay-repair-20261005
export PATH="$HOME/.rustup/toolchains/1.96.1-x86_64-unknown-linux-gnu/bin:$PATH"
/nix/store/g7skjk9lrdnshaxd7px62bchq6yg0bbh-bun-1.3.13/bin/bun \
  ~/.codex/skills/machine-capacity-distilled/scripts/machine-capacity.mjs run \
  --class moderate --owner codex:enigo-delay-proof --timeout-seconds 600 -- \
  nix-shell -p stdenv.cc pkg-config libxkbcommon libx11 libxtst libxi xorg-server xterm xdotool \
  --run 'set -e; cargo test --jobs 2 --lib keymap::; cargo build --jobs 2 --example enigo_text_probe; bash /home/tom/code/enigo/worktrees/enigo-delay-repair-20261005/examples/run_text_probe.sh'
```
