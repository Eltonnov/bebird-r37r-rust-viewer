# R37R wire protocol — observed interoperability notes

**Scope:** tested with one Bebird R37R device only. These values were corroborated by local tests and analysis of the user's legally obtained Android application. No vendor source code is included.

| Function | Destination | Payload (hex) | Notes |
| --- | --- | --- | --- |
| Start MJPEG | 192.168.5.1:58080 UDP | `20 36` | Send from the same UDP socket that will receive video |
| Stop MJPEG | 192.168.5.1:58080 UDP | `20 37` | Graceful stop |
| Battery query | 192.168.5.1:58090 UDP | `66 3A` | Example response `00 01 00 2F`: status=1, value=47 (candidate percent) |
| LED query | 192.168.5.1:58090 UDP | `66 3C FE` | Timeout is allowed; initial value can be unknown |
| LED set | 192.168.5.1:58090 UDP | `66 3C XX` | XX = requested intensity 0–100; observed working |

The response format for video, on the tested R37R:

- UDP datagram begins with a **four-byte** header.
- Byte 0: frame group ID (`u8`); byte 2: packet index (`u8`, starting at 1); bytes 4 onward: JPEG payload.
- First JPEG packet starts `FF D8`; frame ends in `FF D9`.
- Example: `00 00 01 08 FF D8 FF E0 ...`; a later frame begins `01 00 01 08 FF D8 ...`.
- Implementation uses an ordered packet map, discards missing packets, checks JPEG start/end markers, limits frame size to 512 KiB, and discards incomplete frames.
- Header bytes 1 and 3 may carry additional status/angle information and are not interpreted for display in the current viewer.

**Caveats:** a simple end-marker check is not proof that an arbitrary JPEG is fully valid; the image decoder must also succeed. Frames can be lost under fast motion, reflections or RF interference. The current parser is deliberately conservative and does not handle every model/firmware variant.

Unknowns: device discovery behavior across models, advanced framing variants, additional LED feedback semantics, motion sensor/rotation, firmware differences, stream quality controls.
