#include <array>
#include <cerrno>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <iostream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <system_error>
#include <utility>

#include <fcntl.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

namespace {

constexpr std::size_t buffer_size = 4096;
constexpr std::size_t header_size = 4;
constexpr std::size_t max_payload = buffer_size - header_size;
constexpr std::string_view greeting = "hello from the C++ client";
constexpr std::string_view expected_reply = "HELLO FROM THE C++ CLIENT";
static_assert(greeting.size() <= max_payload);

[[noreturn]] void system_error(const char *operation) {
  throw std::system_error(errno, std::generic_category(), operation);
}

class Fd {
public:
  explicit Fd(int value) : value_(value) {}
  Fd(const Fd &) = delete;
  Fd &operator=(const Fd &) = delete;
  Fd(Fd &&other) noexcept : value_(std::exchange(other.value_, -1)) {}
  ~Fd() {
    if (value_ >= 0 && ::close(value_) < 0) {
      std::cerr << "memfd-client: close: " << std::strerror(errno) << '\n';
    }
  }
  int get() const { return value_; }

private:
  int value_;
};

class Mapping {
public:
  explicit Mapping(const Fd &fd)
      : address_(::mmap(nullptr, buffer_size, PROT_READ | PROT_WRITE,
                        MAP_SHARED, fd.get(), 0)) {
    if (address_ == MAP_FAILED) {
      system_error("mmap");
    }
  }
  Mapping(const Mapping &) = delete;
  Mapping &operator=(const Mapping &) = delete;
  ~Mapping() {
    if (::munmap(address_, buffer_size) < 0) {
      std::cerr << "memfd-client: munmap: " << std::strerror(errno) << '\n';
    }
  }
  unsigned char *data() const { return static_cast<unsigned char *>(address_); }

private:
  void *address_;
};

Fd connect_to_server(const std::string &path) {
  sockaddr_un address{};
  address.sun_family = AF_UNIX;
  if (path.empty() || path.size() >= sizeof(address.sun_path)) {
    throw std::runtime_error("socket path is empty or too long");
  }
  std::memcpy(address.sun_path, path.c_str(), path.size() + 1);
  Fd socket(::socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0));
  if (socket.get() < 0) {
    system_error("socket");
  }
  const auto length =
      static_cast<socklen_t>(offsetof(sockaddr_un, sun_path) + path.size() + 1);
  int result;
  do {
    result = ::connect(socket.get(),
                       reinterpret_cast<const sockaddr *>(&address), length);
  } while (result < 0 && errno == EINTR);
  if (result < 0 && errno != EISCONN) {
    system_error("connect");
  }
  return socket;
}

void send_control(const Fd &socket, char message) {
  ssize_t count;
  do {
    count = ::send(socket.get(), &message, 1, MSG_NOSIGNAL);
  } while (count < 0 && errno == EINTR);
  if (count < 0) {
    system_error("send control");
  }
  if (count != 1) {
    throw std::runtime_error("short control write");
  }
}

void expect_done(const Fd &socket) {
  char message{};
  ssize_t count;
  do {
    count = ::recv(socket.get(), &message, 1, 0);
  } while (count < 0 && errno == EINTR);
  if (count < 0) {
    system_error("receive completion");
  }
  if (count == 0) {
    throw std::runtime_error("server disconnected before completion");
  }
  if (message != 'D') {
    throw std::runtime_error("expected completion control 'D'");
  }
}

Fd receive_buffer(const Fd &socket) {
  alignas(cmsghdr) std::array<unsigned char, CMSG_SPACE(sizeof(int))> control{};
  char marker{};
  iovec data{&marker, 1};
  msghdr message{};
  message.msg_iov = &data;
  message.msg_iovlen = 1;
  message.msg_control = control.data();
  message.msg_controllen = control.size();
  ssize_t count;
  do {
    count = ::recvmsg(socket.get(), &message, MSG_CMSG_CLOEXEC);
  } while (count < 0 && errno == EINTR);
  if (count < 0) {
    system_error("receive buffer");
  }

  auto *header = CMSG_FIRSTHDR(&message);
  int descriptor = -1;
  const bool has_descriptor = header != nullptr &&
                              header->cmsg_level == SOL_SOCKET &&
                              header->cmsg_type == SCM_RIGHTS &&
                              header->cmsg_len >= CMSG_LEN(sizeof(descriptor));
  if (has_descriptor) {
    std::memcpy(&descriptor, CMSG_DATA(header), sizeof(descriptor));
  }
  Fd fd(descriptor);
  if ((message.msg_flags & (MSG_CTRUNC | MSG_TRUNC)) != 0) {
    throw std::runtime_error("truncated buffer descriptor message");
  }
  if (count != 1 || marker != 'F' || !has_descriptor ||
      header->cmsg_len != CMSG_LEN(sizeof(descriptor)) ||
      CMSG_NXTHDR(&message, header) != nullptr) {
    throw std::runtime_error(
        "expected control 'F' with exactly one buffer descriptor");
  }
  return fd;
}

void validate_buffer(const Fd &fd) {
  struct stat status{};
  if (::fstat(fd.get(), &status) < 0) {
    system_error("fstat buffer");
  }
  if (!S_ISREG(status.st_mode) ||
      status.st_size != static_cast<off_t>(buffer_size)) {
    throw std::runtime_error("buffer must be a 4096-byte regular file");
  }
  const int seals = ::fcntl(fd.get(), F_GET_SEALS);
  if (seals < 0) {
    system_error("get buffer seals");
  }
  constexpr int required_seals = F_SEAL_SHRINK | F_SEAL_GROW;
  if ((seals & required_seals) != required_seals) {
    throw std::runtime_error("buffer must be sealed against resizing");
  }
}

void write_length(unsigned char *bytes, std::uint32_t length) {
  for (std::size_t index = 0; index < header_size; ++index) {
    bytes[index] = static_cast<unsigned char>((length >> (8 * index)) & 0xffU);
  }
}

std::size_t read_length(const unsigned char *bytes) {
  std::uint32_t length = 0;
  for (std::size_t index = 0; index < header_size; ++index) {
    length |= static_cast<std::uint32_t>(bytes[index]) << (8 * index);
  }
  if (length > max_payload) {
    throw std::runtime_error("response payload length exceeds buffer capacity");
  }
  return length;
}

void run(const std::string &path) {
  auto socket = connect_to_server(path);
  send_control(socket, 'B');
  auto fd = receive_buffer(socket);
  validate_buffer(fd);
  Mapping mapping(fd);
  write_length(mapping.data(), static_cast<std::uint32_t>(greeting.size()));
  std::memcpy(mapping.data() + header_size, greeting.data(), greeting.size());
  std::cout << "Wrote: " << greeting << '\n';
  send_control(socket, 'W');
  expect_done(socket);
  const std::string_view reply(
      reinterpret_cast<const char *>(mapping.data() + header_size),
      read_length(mapping.data()));
  if (reply != expected_reply) {
    throw std::runtime_error("unexpected shared-memory response");
  }
  std::cout << "Read: " << reply << '\n';
}

} // namespace

int main(int argc, char **argv) {
  try {
    if (argc > 2) {
      throw std::runtime_error("usage: memfd-client [socket-path]");
    }
    run(argc == 2 ? argv[1] : "/tmp/memfd-demo.sock");
    return 0;
  } catch (const std::exception &error) {
    std::cerr << "memfd-client: " << error.what() << '\n';
    return 1;
  }
}
