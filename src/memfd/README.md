# Shared-memory Rust/C++ demo

A Linux-only Rust server and C++17 client exchange data through a shared 4 KiB
`memfd_create()` buffer. The server passes its descriptor over a filesystem Unix
domain socket using `SCM_RIGHTS`. Only control bytes travel over the socket:
the client writes a greeting into the mapping, the server reads and uppercases
it in place, and the client reads the result.

## Run

From the repository root:

```sh
nix build .#memfd
```

In one terminal, start the server:

```sh
./result/bin/memfd-server /tmp/memfd-demo.sock
```

In another terminal, run the client, as many times as desired:

```sh
./result/bin/memfd-client /tmp/memfd-demo.sock
```

Expected client output:

```text
Wrote: hello from the C++ client
Read: HELLO FROM THE C++ CLIENT
```

The server prints its socket path and each greeting it reads. It stays running,
serving one connection at a time with a fresh buffer for every connection. Both
programs accept one optional socket path, defaulting to `/tmp/memfd-demo.sock`.

Stop the server with Ctrl-C or SIGTERM; it removes its own socket. It never
automatically unlinks an existing path at startup. After an uncatchable termination
such as SIGKILL, remove the stale socket manually **only after confirming that no
server is using it**. A socket path replaced by another file is not removed.

The socket has owner-only permissions. Use a path inside a private directory if
other local users are untrusted.

## Protocol and ownership

The transport is `SOCK_STREAM`. Each connection performs exactly one exchange:

1. Client sends `B` to request a buffer.
2. Server creates a 4,096-byte memfd, seals it against resizing, and sends `F`
   with exactly one descriptor. The client validates its size and seals.
3. Client maps the descriptor with `MAP_SHARED`, writes the payload, and sends `W`.
4. Server reads the payload, uppercases ASCII letters in place, and sends `D`.
5. Client reads and verifies the result, then disconnects.

The first four mapped bytes contain an unsigned little-endian payload length;
the remaining 4,092 bytes hold the payload, without a terminating NUL. Empty
payloads are valid. Lengths beyond capacity and unexpected control messages are
rejected.

The socket handshakes transfer access to the mapping between the processes.
Neither side accesses the payload during the other's turn. Shared mappings are
coherent without `msync`; no atomics are needed for this single-writer-at-a-time
protocol. Resizing is sealed, but writes remain permitted.

Both programs use five-second read/write timeouts. A failed, disconnected, or
stalled client is reported on the server's stderr and does not stop the accept
loop. Client failures are reported on stderr with a nonzero exit status.
Descriptors and mappings are released after each exchange.

This is a cooperative local demo, not a protocol for hostile peers: a peer that
ignores the ownership handoff could race accesses to the mapping. "Zero-copy"
refers to the IPC payload staying in shared memory, not to initializing that
memory or printing its contents. Concurrent clients, variable buffer allocation,
and authentication are intentionally out of scope.

## Development and checks

Use the repository's Nix development shell for Cargo and the C++ compiler:

```sh
nix develop
cargo build -p memfd
c++ -std=c++17 -O2 -Wall -Wextra -Wpedantic -Werror \
  src/memfd/cpp/client.cpp -o target/memfd-client
cargo test -p memfd
cargo clippy -p memfd --all-targets -- -D warnings
python3 src/memfd/tests/smoke.py target/debug/memfd-server target/memfd-client
```

The smoke test runs both real programs, exercises repeated clients and payload
bounds, checks malformed peers and timeouts, and verifies socket cleanup. It uses
only the Python standard library and isolated temporary socket paths.

```sh
nix fmt
nix flake check
```

On Linux, the flake exposes `packages.memfd`, `checks.memfd`, and
`checks.memfd-roundtrip`. The package includes both executables. These outputs are
omitted on macOS, where the Rust entry point explicitly reports that Linux is
required.
