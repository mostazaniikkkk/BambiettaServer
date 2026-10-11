// Mob persistence: "set" summons and saves; after a restart, "check" looks for them.
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
const [mode, host = '127.0.0.1', port = '25565'] = process.argv.slice(2)
const a = mineflayer.createBot({ host, port: Number(port), username: 'Keeper', version: '1.20.4', auth: 'offline' })
await once(a, 'spawn'); await a.waitForChunksToLoad().catch(() => {})
a.chat('/gamemode creative'); a.chat('/tp 2000.5 101 2000.5'); await sleep(2000)
if (mode === 'set') {
  a.chat('/fill 1990 100 1990 2010 100 2010 stone'); a.chat('/fill 1990 101 1990 2010 104 2010 air'); await sleep(500)
  a.chat('/fill 1988 101 1988 2012 102 1988 stone'); a.chat('/fill 1988 101 2012 2012 102 2012 stone')
  a.chat('/fill 1988 101 1988 1988 102 2012 stone'); a.chat('/fill 2012 101 1988 2012 102 2012 stone'); await sleep(500)
  for (const m of ['cow', 'cow', 'pig', 'sheep']) { a.chat(`/summon ${m}`); await sleep(200) }
  await sleep(1500)
  console.log('antes:', Object.values(a.entities).filter(e => ['cow', 'pig', 'sheep', 'chicken'].includes(e.name)).map(e => e.name).sort().join(','))
  a.chat('/save-all'); await sleep(800)
} else {
  await sleep(3000)
  console.log('después:', Object.values(a.entities).filter(e => ['cow', 'pig', 'sheep', 'chicken'].includes(e.name)).map(e => e.name).sort().join(','))
}
a.quit(); await sleep(300); process.exit(0)
