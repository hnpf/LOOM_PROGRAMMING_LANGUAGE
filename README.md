# The Loom Programming Language!

Welcome to **Loom**, a modern, concurrent, and shell-aware programming language designed for building powerful developer tools, command-line utilities, and rapid prototyping. Loom blends the expressiveness of Python, the concurrency of Go, and the shell scripting capabilities of Bash into a single, cohesive language.

## Key Features

*   **Task-Based Concurrency**: Native support for lightweight tasks and structured concurrency.
*   **Shell Integration**: First-class citizen. Execute shell commands, pipe output, and handle exit codes seamlessly with interpolation.
*   **Frame-Based Objects**: A flexible object system using "Frames" (structs) and "Act" blocks (methods).
*   **Safe Execution**: Strong type checking (compiled via `weave`) and robust error handling (`Result<T>`, `Option<T>`).
*   **Interpret or Compile**: Use `loom run` for rapid iteration or `loom weave` to compile to a standalone binary for performance and distribution.

## Getting Started

### Installation


If you wish to quickly install via cURL, you can do so:

```bash
curl -s https://virex.lol/loom/install.sh | bash
```

Loom is built in Rust. To install, make sure you have Rust and Cargo installed, then clone this repository and build:

```bash
git clone https://github.com/virex/loom-programming-language.git
cd loom-programming-language
./build_and_install.sh
```

The binary will be located at `target/release/loom-lang`, and `$HOME/.local/bin` if using the installer.

### Usage

**Running a script:**

Not in path:

```bash
cargo run -- program.lm
```

Installed:

````bash
loom program.lm
````

**Compiling to a binary:**

Not in path:

```bash
cargo run -- weave program.lm -o binary
./binary
```

Installed:

```bash
loom weave program.lm -o binary
./binary
```

## Quick Example

```loom
// A simple Loom shell focused program

act main() {
    print("testing basic shell...")
    let res = $ "echo hello world"
    print("stdout: " + res.stdout)
    print("status: " + res.status)

    print("testing interpolation...")
    let name = "loom"
    let cmd = "echo hello {name}"
    let res2 = $ cmd
    print("stdout: " + res2.stdout)

    print("testing stderr and error status...")
    let res3 = $ "ls non_existent_file_12345"
    print("status: " + res3.status)
    print("stderr: " + res3.stderr)

    print("testing complex expression...")
    let res4 = $ ("echo " + "joined " + "string")
    print("stdout: " + res4.stdout)
}

main()
```

## Documentation

*   [Tutorial: Build a System Monitor](docs/tutorial.html): A step-by-step guide to building a real CLI tool.
*   [Language Guide](docs/guide.html): Syntax, Types, Control Flow, Frames, and Functions.
*   [Shell Scripting](docs/shell.html): Using the `$` operator, interpolation, and `ShellOutput`.
*   [Concurrency](docs/concurrency.html): Tasks, `spawn`, and `await`.

## Benchmarks

Loom is designed for speed where it matters most—in shell operations and string handling. The `weave` compiler provides a significant performance boost for computational tasks.

| Test | Interpreted | Weaved (Native) | Improvement | Notes |
| :--- | :--- | :--- | :--- | :--- |
| **Fibonacci (35)** | ~61,051ms | **~1,209ms** | **~50.5x 🚀** | Deep recursion overhead reduction |
| **String Stress** | ~627ms | **~305ms** | ~2.0x | 100,000 sequential concatenations |
| **Shell Stress** | ~3,449ms | ~3,392ms | Negligible | Bottlenecked by OS process spawning |
| **Sieve (1M)** | ~5,807ms | **~840ms** | ~6.9x | Array access and memory optimization |

*Benchmarks conducted on a modern Linux environment. Weaved results are compiled to a standalone Rust binary.*

## License

MIT License. See `LICENSE` for details.
