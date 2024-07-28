## Minimal reproducible example for https://github.com/ivmarkov/edge-net/issues/20
When making a request with a client that closes the connection such as curl, `Ok(0)` is returned from the `Read` implementation `esp-wifi`, and `Err(Error::IncompleteHeaders)` is returned.  
When making a request with a client that keeps the connection open, such as Firefox, the `Read` implementation from `esp-wifi` waits for data until timeout.

### Flash with: 

**esp32**
```bash
SSID=<SSID> PASSWORD=<PASSWD> cargo +esp esp32 --release
```

**esp32c3**
```bash
SSID=<SSID> PASSWORD=<PASSWD> cargo +esp esp32c3 --release
```

**esp32s2**
```bash
SSID=<SSID> PASSWORD=<PASSWD> cargo +esp esp32s2 --release
```

**esp32s3**
```bash
SSID=<SSID> PASSWORD=<PASSWD> cargo +esp esp32s3 --release
```
