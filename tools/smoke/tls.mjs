// Test of wss:// (TLS) on the same port, alongside TCP and unencrypted ws://.
// Requires "tls-self-signed": true (or tls-cert/tls-key). Usage: node tls.mjs [host] [port]
import mc from 'minecraft-protocol'
import mineflayer from 'mineflayer'
import WebSocket, { createWebSocketStream } from 'ws'
import https from 'node:https'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'

const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failures = 0
const check = (ok, msg) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${msg}`); if (!ok) failures++ }

// HTTPS page (what the browser shows after accepting the certificate).
const page = await new Promise(resolve => https.get({ host, port, path: '/', rejectUnauthorized: false }, res => {
  let body = ''; res.on('data', d => { body += d }); res.on('end', () => resolve({ status: res.statusCode, body }))
}).on('error', e => resolve({ status: 0, body: e.message })))
check(page.status === 200 && page.body.includes('Bambietta'), `https://${host}:${port}/ responde la página de estado`)

const bot = async (username, url) => {
  const ws = new WebSocket(url, { rejectUnauthorized: false })
  await once(ws, 'open')
  const stream = createWebSocketStream(ws)
  const connect = client => { client.setSocket(stream); setImmediate(() => client.emit('connect')) }
  return mineflayer.createBot({ host, port, username, version: '1.20.4', auth: 'offline', connect })
}

const secure = await bot('WssBot', `wss://${host}:${port}`)
const plain = await bot('WsBot', `ws://${host}:${port}`)
const tcp = mineflayer.createBot({ host, port, username: 'TcpBot', version: '1.20.4', auth: 'offline' })
await Promise.all([once(secure, 'spawn'), once(plain, 'spawn'), once(tcp, 'spawn')])
check(true, 'los tres transportes (wss, ws y tcp) entran en el mismo puerto')
await sleep(1000)
const got = new Promise(resolve => tcp.on('messagestr', m => m.includes('hola cifrado') && resolve(m)))
secure.chat('hola cifrado')
check(await Promise.race([got.then(() => true), sleep(3000).then(() => false)]), 'chat de wss llega al cliente TCP')
tcp.chat('/list'); await sleep(500)
check(Object.keys(secure.players).length === 3, `el cliente wss ve a los 3 jugadores (${Object.keys(secure.players).join(', ')})`)

// Client that sends packets nonstop: the server kicks it.
const spam = mc.createClient({ host, port, username: 'Flooder', version: '1.20.4', auth: 'offline' })
spam.on('error', () => {}) // the server closes the connection mid-write
await once(spam, 'login')
const kicked = new Promise(resolve => spam.on('end', resolve))
for (let i = 0; i < 2000; i++) spam.write('flying', { onGround: true })
check(await Promise.race([kicked.then(() => true), sleep(4000).then(() => false)]), 'un cliente que inunda de paquetes es expulsado')

secure.quit(); plain.quit(); tcp.quit(); await sleep(300)
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
