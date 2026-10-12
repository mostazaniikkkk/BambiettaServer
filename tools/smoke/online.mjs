// Online mode (run with "online-mode": true): a client without a Mojang session goes through
// the encrypted login and is refused once Mojang does not confirm it (which proves the RSA
// exchange, AES/CFB8 both ways and the HTTPS query to the session server); WebSocket
// clients still join in offline mode.
// Usage: node online.mjs [host] [port]
import mineflayer from 'mineflayer'
import WebSocket, { createWebSocketStream } from 'ws'
const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failed = 0
const check = (ok, what) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${what}`); if (!ok) failed++ }
const attempt = (opts) => new Promise(resolve => {
  const b = mineflayer.createBot({ host, port, username: 'Cracked', version: '1.20.4', auth: 'offline', ...opts })
  b._client.on('registry_data', p => b.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
  b.once('spawn', () => resolve({ ok: true, bot: b }))
  b.once('kicked', r => resolve({ ok: false, reason: typeof r === 'string' ? r : JSON.stringify(r) }))
  b.once('end', r => setTimeout(() => resolve({ ok: false, reason: `end ${r}` }), 500))
  b.on('error', () => {})
  setTimeout(() => resolve({ ok: false, reason: 'timeout' }), 15000)
})

const tcp = await attempt({})
check(!tcp.ok && /verificar tu cuenta de Minecraft/.test(tcp.reason), `por TCP sin cuenta de Mojang se rechaza tras el login cifrado (${tcp.reason?.slice(0, 90)})`)

const ws = new WebSocket(`ws://${host}:${port}`)
const stream = createWebSocketStream(ws)
const viaWs = await attempt({ username: 'Browser', connect: c => { c.setSocket(stream); setImmediate(() => c.emit('connect')) } })
check(viaWs.ok, 'por WebSocket entra en modo offline')
viaWs.bot?.quit()
console.log(failed ? `\n${failed} fallaron` : '\nTodo OK')
process.exit(failed ? 1 : 0)
