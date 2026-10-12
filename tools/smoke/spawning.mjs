// Monsters spawn in the dark at any height: in caves even at midday, and not on the sunlit
// surface. Needs the "noise" generator (caves). Usage: node spawning.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failed = 0
const check = (ok, what) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${what}`); if (!ok) failed++ }
const b = mineflayer.createBot({ host, port, username: 'Caver', version: '1.20.4', auth: 'offline' })
b._client.on('registry_data', p => b.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
await once(b, 'spawn'); await sleep(1500)
const monsters = ['zombie', 'skeleton', 'creeper', 'spider', 'enderman']
b.chat('/gamemode creative'); b.chat('/time set 6000'); b.chat('/weather clear')
// Deep underground, in the middle of the caves.
b.chat('/tp 300 -20 300'); await sleep(4000)
const seenDeep = new Set(), seenTop = new Set()
for (let i = 0; i < 60; i++) {
  for (const e of Object.values(b.entities)) {
    if (!monsters.includes(e.name)) continue
    if (e.position.y < 40) seenDeep.add(e.id); else seenTop.add(e.id)
  }
  await sleep(1000)
}
check(seenDeep.size >= 2, `de día aparecen monstruos en las cuevas (${seenDeep.size})`)
check(seenTop.size === 0, `y no en la superficie al sol (${seenTop.size})`)
b.quit()
console.log(failed ? `\n${failed} fallaron` : '\nTodo OK')
process.exit(failed ? 1 : 0)
