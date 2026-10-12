// Samples blocks around spawn (on vanilla or Bambietta) so they can be compared.
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
import { Vec3 } from 'vec3'
const [host, port, save] = process.argv.slice(2)
const a = mineflayer.createBot({ host, port: Number(port), username: 'Checker', version: '1.20.4', auth: 'offline' })
a._client.on('registry_data', p => a.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
await once(a, 'spawn'); await a.waitForChunksToLoad().catch(() => {}); await sleep(1000)
a.chat('/tp 8 120 8'); await sleep(1500); await a.waitForChunksToLoad().catch(() => {})
const out = []
for (let x = -8; x < 24; x += 3) for (let z = -8; z < 24; z += 3) for (const y of [40, 55, 60, 64, 70, 80]) out.push(a.blockAt(new Vec3(x, y, z))?.name ?? '?')
if (save === 'save') { a.chat('/save-all flush'); await sleep(3000) }
console.log(JSON.stringify(out))
a.quit(); await sleep(300); process.exit(0)
