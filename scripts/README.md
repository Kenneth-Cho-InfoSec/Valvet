# Valvet oracle comparison

`compare_oracles.py` compares a port's actual output with an expected oracle
using the three-line label protocol:

```
label
=
value
```

Numeric values are compared by binary64 bit pattern. `--one-ulp` accepts a
single representable step between same-sign values. The command exits 0 only
when there are no missing, extra, or malformed records and every numeric
difference is permitted.

Port artifacts are external to this repository. A clean checkout can validate
the checker itself, but it cannot verify a port until an oracle root is
supplied:

```sh
python3 scripts/compare_oracles.py \
  --expected /path/to/port08.expected \
  --actual /path/to/port08.actual \
  --port port08 --one-ulp --json result.json
```

For a manifest-driven run, point `--oracles-root` at a directory containing
`manifest.json` (schema in `oracle_schema.json`) and the referenced files:

```sh
python3 scripts/compare_oracles.py --all --oracles-root /path/to/root
```

Run the checker tests with:

```sh
python3 -m unittest discover -s scripts/tests -v
```
