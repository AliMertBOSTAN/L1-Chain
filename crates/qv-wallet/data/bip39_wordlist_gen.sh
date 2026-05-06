#!/bin/bash
# Generate BIP-39 wordlist inline Rust code (2048 words)
# Source: https://github.com/trezor/python-mnemonic/blob/master/vectors.json
cat << 'WORDLIST' > bip39_wordlist.txt
abandon ability able about above absent absorb abstract abuse access accident account accuse achieve acid acoustic acquire across act action actor actions actual
WORDLIST
