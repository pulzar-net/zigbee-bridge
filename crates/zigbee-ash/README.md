# zigbee-ash

ASHv2, the Asynchronous Serial Host protocol that carries EZSP between a
host and a Silicon Labs EmberZNet network co-processor (Silicon Labs
UG101).

- `frame`: frame encoding and decoding, CRC-CCITT, byte stuffing, data
  randomization, and a bounded stream `frame::Decoder`.
- `conn`: the link as a sans-IO state machine: RST/RSTACK, frame
  numbering, piggybacked and explicit ACKs, NAK on loss or corruption,
  go-back-N retransmission, a sliding window and UG101's adaptive ACK
  timer. It serves both ends of the link.
- `transport` (feature `tokio`): a task that runs a connection over any
  `AsyncRead + AsyncWrite`, such as a serial port or a TCP socket.

Part of [zigbee-bridge](https://github.com/pulzar-net/zigbee-bridge), maintained by
[Pulzr](https://pulzr.net). See the [user guide](https://oss.pulzr.net/zigbee-bridge/), the
[library guide](https://oss.pulzr.net/zigbee-bridge/library/index.html) and the
[API documentation](https://oss.pulzr.net/zigbee-bridge/api/zigbee_ash/index.html).

License: MIT.
