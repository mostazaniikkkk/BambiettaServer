// Legitimate survival movement: it must not trigger any anti-cheat corrections.
// Usage: node movement.mjs [host] [port]   (then look for "corrección de posición" in the log)
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'

const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
const bot = mineflayer.createBot({ host, port, username: 'Runner', version: '1.20.4', auth: 'offline' })
bot._client.on('registry_data', p => bot.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
await once(bot, 'spawn'); await bot.waitForChunksToLoad(); await sleep(500)
bot.chat('/gamemode survival'); bot.chat('/heal'); await sleep(500)
let forced = 0
bot.on('forcedMove', () => forced++)
await sleep(300); forced = 0

const run = async (label, yaw, controls, ms) => {
  await bot.look(yaw, 0, true)
  for (const c of controls) bot.setControlState(c, true)
  await sleep(ms)
  bot.clearControlStates()
  await sleep(400)
  console.log(`${label.padEnd(22)} correcciones acumuladas: ${forced}  pos ${bot.entity.position.floored()}`)
}
await run('caminar', 0, ['forward'], 3000)
await run('correr', Math.PI / 2, ['forward', 'sprint'], 3000)
await run('sprint + salto', Math.PI, ['forward', 'sprint', 'jump'], 4000)
await run('saltar en el sitio', 0, ['jump'], 2000)
await run('agachado', -Math.PI / 2, ['forward', 'sneak'], 2000)
await run('sprint + salto (otra)', Math.PI / 4, ['forward', 'sprint', 'jump'], 4000)
const p = bot.entity.position
bot.chat(`/tp ${p.x} ${p.y + 6} ${p.z}`); await sleep(2500)
console.log(`${'caída de 6 bloques'.padEnd(22)} correcciones acumuladas: ${forced - 1} (sin contar el /tp)  vida ${bot.health}`)
bot.quit(); await sleep(300)
process.exit(forced > 1 ? 1 : 0)
