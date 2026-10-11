// Minimal UDP Query client (GameSpy4). Usage: node query.mjs [host] [port]
import dgram from 'node:dgram'
import { once } from 'node:events'
const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
const sock = dgram.createSocket('udp4')
const session = Buffer.from([0, 0, 0, 1])
sock.send(Buffer.concat([Buffer.from([0xfe, 0xfd, 9]), session]), port, host)
const [hs] = await once(sock, 'message')
const token = Number(hs.subarray(5, hs.length - 1).toString())
const tb = Buffer.alloc(4); tb.writeInt32BE(token)
sock.send(Buffer.concat([Buffer.from([0xfe, 0xfd, 0]), session, tb, Buffer.alloc(4)]), port, host)
const [full] = await once(sock, 'message')
const parts = full.subarray(16).toString('utf8').split('\0')
const end = parts.indexOf('')
const kv = {}
for (let i = 0; i < end; i += 2) kv[parts[i]] = parts[i + 1]
console.log(kv, 'jugadores:', parts.slice(end + 3).filter(Boolean))
sock.close()
