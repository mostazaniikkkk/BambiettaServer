// Connected to the VANILLA server, checks that chunks written by Bambietta are read correctly.
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
import { Vec3 } from 'vec3'
const [host = '127.0.0.1', port = '25590'] = process.argv.slice(2)
const a = mineflayer.createBot({ host, port: Number(port), username: 'Checker', version: '1.20.4', auth: 'offline' })
a._client.on('registry_data', p => a.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
a.on('messagestr', m => { if (/Unknown|denied|permission/i.test(m)) console.log('  chat:', m) })
await once(a, 'spawn'); await sleep(2000)
// On vanilla the bot is not op: looking from spawn is not enough, it has to move closer.
a.chat('/tp 2000 101 2000')
await sleep(1500)
console.log('posición', a.entity.position.toString())
await a.waitForChunksToLoad().catch(() => {})
const at = (x, y, z) => a.blockAt(new Vec3(x, y, z))?.name
const checks = [
  ['plataforma de piedra (2000,100,2000)', at(2000, 100, 2000) === 'stone'],
  ['marco del portal del Nether (1999,103,2005)', at(1999, 103, 2005) === 'obsidian'],
  ['portal encendido (2000,102,2005)', at(2000, 102, 2005) === 'nether_portal'],
  ['marcos del portal del End (1994,101,1993)', at(1994, 101, 1993) === 'end_portal_frame'],
  ['portal del End activo (1995,101,1995)', at(1995, 101, 1995) === 'end_portal'],
]
for (const [msg, ok] of checks) console.log(`${ok ? 'OK  ' : 'FAIL'} vanilla lee: ${msg}`)
a.quit(); await sleep(300); process.exit(checks.every(c => c[1]) ? 0 : 1)
