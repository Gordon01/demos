import argparse
import array
import contextlib
import fcntl
import mmap
import os
from pathlib import Path
import signal
import socket
import stat
import struct
import subprocess
import tempfile
import time
import unittest


BUFFER_SIZE = 4096
MAX_PAYLOAD = BUFFER_SIZE - 4
GREETING = b"hello from the C++ client"
REPLY = GREETING.upper()


def stop_process(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=3)
            raise AssertionError("child did not stop after SIGTERM")


@contextlib.contextmanager
def shared_buffer(size=BUFFER_SIZE, sealed=True):
    fd = os.memfd_create("memfd-test", os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
    try:
        os.ftruncate(fd, size)
        if sealed:
            fcntl.fcntl(fd, fcntl.F_ADD_SEALS, fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_GROW)
        yield fd
    finally:
        os.close(fd)


class DemoTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix="memfd-")
        self.addCleanup(directory.cleanup)
        self.directory = Path(directory.name)
        self.path = self.directory / "demo.sock"
        self.server_count = 0

    def start_server(self):
        self.server_count += 1
        log_path = self.directory / f"server-{self.server_count}.log"
        log = log_path.open("wb")
        self.addCleanup(log.close)
        process = subprocess.Popen(
            [self.server_binary, str(self.path)],
            stdout=log,
            stderr=subprocess.STDOUT,
        )
        self.addCleanup(stop_process, process)
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            if process.poll() is not None:
                self.fail(f"server exited during startup:\n{log_path.read_text()}")
            if f"Listening on {self.path}\n" in log_path.read_text():
                self.assertEqual(stat.S_IMODE(self.path.stat().st_mode), 0o600)
                return process, log_path
            time.sleep(0.01)
        self.fail(f"server did not become ready:\n{log_path.read_text()}")

    def client(self, success=True):
        result = subprocess.run(
            [self.client_binary, str(self.path)],
            capture_output=True,
            text=True,
            timeout=8,
        )
        if success:
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                result.stdout,
                f"Wrote: {GREETING.decode()}\nRead: {REPLY.decode()}\n",
            )
            self.assertEqual(result.stderr, "")
        else:
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("memfd-client:", result.stderr)
        return result

    @contextlib.contextmanager
    def connection(self):
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
            connection.settimeout(8)
            connection.connect(str(self.path))
            yield connection

    @contextlib.contextmanager
    def buffer_client(self):
        with self.connection() as connection, contextlib.ExitStack() as resources:
            connection.sendall(b"B")
            message, ancillary, flags, _ = connection.recvmsg(
                1, socket.CMSG_SPACE(8 * array.array("i").itemsize), socket.MSG_CMSG_CLOEXEC
            )
            descriptors = []
            for level, kind, data in ancillary:
                self.assertEqual((level, kind), (socket.SOL_SOCKET, socket.SCM_RIGHTS))
                received = array.array("i")
                received.frombytes(data)
                for fd in received:
                    resources.callback(os.close, fd)
                    descriptors.append(fd)
            self.assertEqual(message, b"F")
            self.assertEqual(flags & (socket.MSG_CTRUNC | socket.MSG_TRUNC), 0)
            self.assertEqual(len(descriptors), 1)
            fd = descriptors[0]
            self.assertEqual(os.fstat(fd).st_size, BUFFER_SIZE)
            required_seals = fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK
            self.assertEqual(
                fcntl.fcntl(fd, fcntl.F_GET_SEALS) & required_seals, required_seals
            )
            self.assertTrue(fcntl.fcntl(fd, fcntl.F_GETFD) & fcntl.FD_CLOEXEC)
            mapping = resources.enter_context(mmap.mmap(fd, BUFFER_SIZE))
            yield connection, mapping

    @contextlib.contextmanager
    def fake_server(self):
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
            listener.settimeout(8)
            listener.bind(str(self.path))
            try:
                listener.listen(1)
                process = subprocess.Popen(
                    [self.client_binary, str(self.path)],
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                )
                try:
                    connection, _ = listener.accept()
                    with connection:
                        connection.settimeout(8)
                        self.assertEqual(connection.recv(1), b"B")
                        yield connection, process
                finally:
                    stop_process(process)
                    process.stdout.close()
                    process.stderr.close()
            finally:
                self.path.unlink()

    def assert_client_failure(self, process, message):
        stdout, stderr = process.communicate(timeout=8)
        self.assertNotEqual(process.returncode, 0, stdout)
        self.assertIn("memfd-client:", stderr)
        self.assertIn(message, stderr)

    def test_repeated_real_clients(self):
        _, log_path = self.start_server()
        self.client()
        self.client()
        self.assertEqual(log_path.read_text().count(f"Read: {GREETING.decode()}\n"), 2)

    def test_payload_boundaries(self):
        self.start_server()
        for payload in [b"", b"a" * MAX_PAYLOAD]:
            with self.subTest(length=len(payload)), self.buffer_client() as (peer, mapping):
                mapping[:4] = struct.pack("<I", len(payload))
                mapping[4 : 4 + len(payload)] = payload
                peer.sendall(b"W")
                self.assertEqual(peer.recv(1), b"D")
                self.assertEqual(struct.unpack("<I", mapping[:4])[0], len(payload))
                self.assertEqual(mapping[4 : 4 + len(payload)], payload.upper())
        self.client()

    def test_invalid_lengths_do_not_stop_server(self):
        self.start_server()
        for length in [MAX_PAYLOAD + 1, 0xFFFFFFFF]:
            with self.subTest(length=length), self.buffer_client() as (peer, mapping):
                mapping[:4] = struct.pack("<I", length)
                peer.sendall(b"W")
                self.assertEqual(peer.recv(1), b"")
            self.client()

    def test_bad_controls_and_disconnects_do_not_stop_server(self):
        self.start_server()
        with self.connection() as peer:
            peer.sendall(b"X")
            self.assertEqual(peer.recv(1), b"")
        with self.connection():
            pass
        with self.buffer_client():
            pass
        with self.buffer_client() as (peer, _):
            peer.sendall(b"X")
            self.assertEqual(peer.recv(1), b"")
        self.client()

    def test_server_timeout_and_recovery(self):
        _, log_path = self.start_server()
        with self.buffer_client() as (peer, _):
            start = time.monotonic()
            self.assertEqual(peer.recv(1), b"")
            elapsed = time.monotonic() - start
            self.assertGreaterEqual(elapsed, 4.5)
            self.assertLess(elapsed, 8)
        self.assertIn("timed out after 5 seconds", log_path.read_text())
        self.client()

    def test_existing_file_is_preserved(self):
        self.path.write_text("keep this file")
        result = subprocess.run(
            [self.server_binary, str(self.path)],
            capture_output=True,
            text=True,
            timeout=8,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("binding", result.stderr)
        self.assertEqual(self.path.read_text(), "keep this file")

    def test_existing_listener_is_preserved(self):
        self.start_server()
        result = subprocess.run(
            [self.server_binary, str(self.path)],
            capture_output=True,
            text=True,
            timeout=8,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("binding", result.stderr)
        self.client()

    def test_shutdown_cleanup_and_restart(self):
        for signum in [signal.SIGINT, signal.SIGTERM]:
            with self.subTest(signal=signum):
                process, log_path = self.start_server()
                self.client()
                process.send_signal(signum)
                self.assertEqual(process.wait(timeout=3), 0, log_path.read_text())
                self.assertFalse(self.path.exists())

    def test_replaced_path_is_preserved(self):
        process, log_path = self.start_server()
        self.path.unlink()
        self.path.write_text("replacement")
        process.terminate()
        self.assertEqual(process.wait(timeout=3), 1)
        self.assertEqual(self.path.read_text(), "replacement")
        self.assertIn("refusing to remove a replaced socket path", log_path.read_text())

    def test_missing_server(self):
        self.client(success=False)

    def test_client_rejects_bad_buffers(self):
        cases = [
            ("missing", b"F", 0, BUFFER_SIZE, True, "exactly one"),
            ("extra", b"F", 2, BUFFER_SIZE, True, "exactly one"),
            ("truncated", b"F", 8, BUFFER_SIZE, True, "truncated"),
            ("marker", b"X", 1, BUFFER_SIZE, True, "expected control"),
            ("size", b"F", 1, BUFFER_SIZE - 1, True, "4096-byte"),
            ("unsealed", b"F", 1, BUFFER_SIZE, False, "sealed against resizing"),
        ]
        for name, marker, count, size, sealed, error in cases:
            with self.subTest(case=name), self.fake_server() as (peer, process):
                with shared_buffer(size, sealed) as fd:
                    ancillary = (
                        [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array("i", [fd] * count))]
                        if count
                        else []
                    )
                    self.assertEqual(peer.sendmsg([marker], ancillary), 1)
                    self.assert_client_failure(process, error)

    def test_client_rejects_bad_completion(self):
        cases = [
            ("control", b"X", len(REPLY), REPLY, "expected completion"),
            ("length", b"D", 0xFFFFFFFF, REPLY, "exceeds buffer capacity"),
            ("payload", b"D", 5, b"wrong", "unexpected shared-memory response"),
            ("disconnect", b"", 0, b"", "disconnected before completion"),
        ]
        for name, control, length, payload, error in cases:
            with self.subTest(case=name), self.fake_server() as (peer, process):
                with shared_buffer() as fd, mmap.mmap(fd, BUFFER_SIZE) as mapping:
                    peer.sendmsg(
                        [b"F"],
                        [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array("i", [fd]))],
                    )
                    self.assertEqual(peer.recv(1), b"W")
                    self.assertEqual(struct.unpack("<I", mapping[:4])[0], len(GREETING))
                    self.assertEqual(mapping[4 : 4 + len(GREETING)], GREETING)
                    mapping[:4] = struct.pack("<I", length)
                    mapping[4 : 4 + len(payload)] = payload
                    if control:
                        peer.sendall(control)
                    else:
                        peer.shutdown(socket.SHUT_RDWR)
                    self.assert_client_failure(process, error)

    def test_client_timeout(self):
        with self.fake_server() as (_, process):
            start = time.monotonic()
            self.assert_client_failure(process, "I/O timed out after 5 seconds")
            elapsed = time.monotonic() - start
            self.assertGreaterEqual(elapsed, 4.5)
            self.assertLess(elapsed, 8)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Test the Rust/C++ memfd demo")
    parser.add_argument("server", type=Path)
    parser.add_argument("client", type=Path)
    args = parser.parse_args()
    DemoTests.server_binary = str(args.server.resolve(strict=True))
    DemoTests.client_binary = str(args.client.resolve(strict=True))
    unittest.main(argv=[__file__], verbosity=2)
