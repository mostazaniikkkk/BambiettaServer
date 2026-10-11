// Bambietta smoke test: one bot over vanilla TCP and another over WebSocket.
// Usage: node smoke.mjs [host] [port]
import mc from 'minecraft-protocol'
import mineflayer from 'mineflayer'
import WebSocket, { createWebSocketStream } from 'ws'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'

const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
const version = '1.20.4'
let failures = 0
const check = (ok, msg) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${msg}`); if (!ok) failures++ }
const withTimeout = (p, ms, what) => Promise.race([p, sleep(ms).then(() => { throw new Error(`timeout: ${what}`) })])

const status = await mc.ping({ host, port })
check(status.version.protocol === 765, `status TCP: ${status.version.name} / ${status.players.online} online`)

// Status over WebSocket (handshake + request + ping by hand)
{
  const client = mc.createClient({ host, port, version, username: 'x', auth: 'offline', connect: () => {} })
  const ws = new WebSocket(`ws://${host}:${port}`)
  await once(ws, 'open')
  client.setSocket(createWebSocketStream(ws))
  client.state = 'handshaking'
  client.write('set_protocol', { protocolVersion: 765, serverHost: host, serverPort: port, nextState: 1 })
  client.state = 'status'
  client.write('ping_start', {})
  const [info] = await withTimeout(once(client, 'server_info'), 5000, 'status ws')
  check(JSON.parse(info.response).version.protocol === 765, 'status WebSocket')
  client.end()
}

function tcpBot (username) {
  return mineflayer.createBot({ host, port, username, version, auth: 'offline' })
}

async function wsBot (username) {
  const ws = new WebSocket(`ws://${host}:${port}`, ['minecraft'])
  await once(ws, 'open')
  const stream = createWebSocketStream(ws)
  // In offline mode minecraft-protocol calls connect() before registering its listeners,
  // so 'connect' is emitted on the next tick.
  const connect = client => { client.setSocket(stream); setImmediate(() => client.emit('connect')) }
  return mineflayer.createBot({ host, port, username, version, auth: 'offline', connect })
}

const a = tcpBot('TcpBot')
const aSpawn = once(a, 'spawn')
const b = await wsBot('WsBot')
const bSpawn = once(b, 'spawn')
for (const bot of [a, b]) bot.on('kicked', r => console.log(bot.username, 'kicked', JSON.stringify(r)))
await withTimeout(Promise.all([aSpawn, bSpawn]), 20000, 'spawn')
check(true, 'ambos bots aparecieron')
// The test builds using the creative inventory.
a.chat('/gamemode creative TcpBot'); b.chat('/gamemode creative WsBot')
await withTimeout(Promise.all([a.waitForChunksToLoad(), b.waitForChunksToLoad()]), 30000, 'chunks')
const below = a.blockAt(a.entity.position.offset(0, -1, 0))
check(below && below.name !== 'air', `bloque bajo TcpBot: ${below?.name} @ ${a.entity.position}`)

await sleep(1000)
check(!!a.players.WsBot?.entity, 'TcpBot ve la entidad de WsBot')
check(!!b.players.TcpBot?.entity, 'WsBot ve la entidad de TcpBot')

// Chat
const got = withTimeout(new Promise(resolve => a.on('messagestr', m => m.includes('hola desde ws') && resolve(m))), 5000, 'chat')
b.chat('hola desde ws')
check((await got).includes('WsBot'), 'chat WS -> TCP')

// Command
const tpDone = withTimeout(once(b, 'forcedMove'), 5000, 'tp')
b.chat('/tp 20 100 20')
await tpDone
check(Math.abs(b.entity.position.x - 20) < 1 && Math.abs(b.entity.position.z - 20) < 1, `/tp mueve a WsBot: ${b.entity.position}`)

// Build: TcpBot (creative) places stone and WsBot receives the change
const Item = (await import('prismarine-item')).default(version)
await a.creative.setInventorySlot(36, new Item(a.registry.itemsByName.stone.id, 1))
a.setQuickBarSlot(0)
await sleep(300)
const ref = a.findBlock({ matching: bl => bl.boundingBox === 'block', maxDistance: 4,
  useExtraInfo: bl => bl.position.distanceTo(a.entity.position.floored()) >= 2 && a.blockAt(bl.position.offset(0, 1, 0))?.name === 'air' })
const targetPos = ref.position.offset(0, 1, 0)
const seen = withTimeout(new Promise(resolve => b.on('blockUpdate', (o, n) => n?.position.equals(targetPos) && resolve(n.name))), 5000, 'blockUpdate').catch(() => 'nada')
await a.placeBlock(ref, { x: 0, y: 1, z: 0 }).catch(e => console.log('placeBlock:', e.message))
check((await seen) === 'stone', `WsBot ve la piedra colocada en ${targetPos} (sobre ${ref.name})`)

// Break
const broken = withTimeout(new Promise(resolve => b.on('blockUpdate', (o, n) => n?.position.equals(targetPos) && n.name === 'air' && resolve(true))), 5000, 'break').catch(() => false)
await a.dig(a.blockAt(targetPos)).catch(e => console.log('dig:', e.message))
check(await broken, 'WsBot ve el bloque roto')

a.quit(); b.quit()
await sleep(500)
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
