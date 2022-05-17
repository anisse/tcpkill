# tcpkill

small tool to open a tcp socket from a given pid and shut it down, killing the connection. Does not need traffic on the socket, relies on getting the socket pidfd and calling shutdown() on it after enabling SO_LINGER(0).

Does not need to have any traffic on the socket, like the original tcpkill. Original idea from [@bradfitz](https://twitter.com/bradfitz/status/1522651333085462528); [his Go PoC](https://go.dev/play/p/UGVZEdt-Sd0)

Build with:
```
cargo build --release
```


# TODO

 * search by connection instead of fd
