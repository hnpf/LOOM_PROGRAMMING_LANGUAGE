#!/bin/bash

# Welcome to the Loom Installer! :)

# lets add the Loom CLI to local path.

set -e

echo "Installing Loom! Please wait.."

cargo build --release # <-- this is just for compilation ensuring code stays actually new

mkdir -p "$HOME/.local/bin"
cp target/release/loom-lang "$HOME/.local/bin/loom" # bin path will change to root dir loom-lang in release install scripts. 


if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then # <-- check if ~/.local/bin is in PATH
    echo "$HOME/.local/bin is not in your PATH."
    echo "Add this to your .bashrc, .zshrc, or whatever you're using:"
    echo 'export PATH="$HOME/.local/bin:$PATH"'
else
    echo "Loom is now installed at $HOME/.local/bin/loom"
fi

echo "You can try running: loom {program}.lm"
echo "have fun with your Loom programming experience!"
