# The Loom Programming Language

Welcome to **Loom**, a modern, concurrent, and shell-aware programming language designed for building powerful developer tools, command-line utilities, and rapid prototyping. Loom blends the expressiveness of Python, the concurrency of Go, and the shell scripting capabilities of Bash into a single, cohesive language.

## Key Features

*   **Task-Based Concurrency**: Native support for lightweight tasks and structured concurrency.
*   **Shell Integration**: First-class citizen. Execute shell commands, pipe output, and handle exit codes seamlessly with interpolation.
*   **Frame-Based Objects**: A flexible object system using "Frames" (structs) and "Act" blocks (methods).
*   **Safe Execution**: Strong type checking (compiled via `weave`) and robust error handling (`Result<T>`, `Option<T>`).
*   **Interpret or Compile**: Use `loom run` for rapid iteration or `loom weave` to compile to a standalone binary for performance and distribution.

## Getting Started

### Installation

Loom is built in Rust. To install, ensure you have Rust and Cargo installed, then clone this repository and build:

```bash
git clone https://github.com/virex/loom-programming-language.git
cd loom-programming-language
cargo build --release
```

The binary will be located at `target/release/loom-lang`.

### Usage

**Running a script:**

```bash
cargo run -- run program.lm
```

**Compiling to a binary:**

```bash
cargo run -- weave program.lm -o binary
./binary
```

## Quick Example

```loom
// A simple Loom script

act main() {
    print("Starting Loom...")
    
    let name = "Developer"
    let greeting = "Hello, " + name
    print(greeting)
    
    // Spawn a concurrent task
    let task = spawn {
        let i = 0
        while i < 3 {
            print("Task working: " + i)
            i = i + 1
        }
        "Done!"
    }
    
    // Execute a shell command
    let res = $ "echo 'Shell integration active'"
    if res.status == 0 {
        print("Shell: " + res.stdout)
    }

    // Await the task
    let result = task.await()
    when result {
        Ok(msg) => print("Task result: " + msg),
        Err(e)  => print("Task failed: " + e),
    }
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
