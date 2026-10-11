// Measures how long the spawn area takes to load (and again after teleporting far away).
import mineflayer from 'mineflayer'
import { once } from 'node:events'
const [host = '127.0.0.1', port = '25565'] = process.argv.slice(2)
let t = Date.now()
const a = mineflayer.createBot({ host, port: Number(port), username: 'Bench', version: '1.20.4', auth: 'offline' })
await once(a, 'spawn'); await a.waitForChunksToLoad().catch(() => {})
console.log(`entrada + chunks: ${Date.now() - t} ms, ${Object.keys(a.world.async.columns).length} columnas`)
a.chat('/gamemode creative')
t = Date.now()
a.chat('/tp 5000 150 5000')
await once(a, 'forcedMove'); await a.waitForChunksToLoad().catch(() => {})
console.log(`tp a zona nueva: ${Date.now() - t} ms`)
a.quit(); setTimeout(() => process.exit(0), 300)
