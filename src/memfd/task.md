Implement a client-server demo using shared memory for zero-copy data exchange.

- **Server:** Rust. Use good quality helper crates when possible.
- **Client:** C++.
- **Transport:** Unix domain socket (UDS).
- **Shared memory:** Use Linux `memfd_create()` to allocate shared memory buffers and `SCM_RIGHTS` to transfer file descriptors over the UDS connection.
- **Protocol:** The client should be able to request a shared memory buffer from the server.
- **Demo:** Demonstrate bidirectional data exchange: both client and server should write to and read from the shared memory buffer.

Keep the implementation simple.
