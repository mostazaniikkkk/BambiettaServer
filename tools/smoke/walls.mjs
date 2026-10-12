// Mobs against walls: they must not hop forever against a wall they cannot jump, and a
// zombie finds its way around one to reach the player, and up a ladder (pathfinding).
// Usage: node walls.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failed = 0
const check = (ok, what) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${what}`); if (!ok) failed++ }
const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end; await sleep(100)) if (f()) return true; return false }
const b = mineflayer.createBot({ host, port, username: 'Walls', version: '1.20.4', auth: 'offline' })
b._client.on('registry_data', p => b.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
await once(b, 'spawn'); await sleep(1500)
const [X, Y, Z] = [6000, 120, 6000]
b.chat('/gamemode creative'); b.chat(`/tp ${X} ${Y + 5} ${Z}`); await sleep(2500)
// Floor, a room closed by a roof (no sun for the zombie) and a 2-block wall around the mobs.
b.chat(`/fill ${X - 10} ${Y - 1} ${Z - 10} ${X + 10} ${Y - 1} ${Z + 10} stone`)
b.chat(`/fill ${X - 10} ${Y} ${Z - 10} ${X + 10} ${Y + 3} ${Z + 10} air`)
b.chat(`/fill ${X - 10} ${Y + 4} ${Z - 10} ${X + 10} ${Y + 4} ${Z + 10} stone`)
b.chat(`/fill ${X - 4} ${Y} ${Z - 4} ${X + 4} ${Y + 1} ${Z + 4} cobblestone`)
b.chat(`/fill ${X - 3} ${Y} ${Z - 3} ${X + 3} ${Y + 1} ${Z + 3} air`)
await sleep(1000)
b.chat(`/tp ${X} ${Y} ${Z}`); await sleep(1000)
b.chat('/summon cow'); b.chat('/summon pig'); b.chat('/summon zombie'); await sleep(500)
// The player stands outside the wall, in survival: the zombie wants to reach it.
b.chat(`/tp ${X + 7} ${Y} ${Z}`); await sleep(500)
b.chat('/gamemode survival'); await sleep(1500)
const mobs = Object.values(b.entities).filter(e => ['cow', 'pig', 'zombie'].includes(e.name) && e.position.distanceTo(b.entity.position) < 15)
check(mobs.length === 3, `3 mobs dentro del corral (${mobs.map(m => m.name)})`)
const air = {}
for (const m of mobs) air[m.name] = 0
for (let t = 0; t < 300; t++) { // 15 s
  for (const m of mobs) if (m.position.y > Y + 0.3) air[m.name]++
  await sleep(50)
}
for (const m of mobs) check(air[m.name] < 40, `${m.name} no salta contra la muralla (${air[m.name]}/300 muestras en el aire)`)
check(mobs.every(m => Math.abs(m.position.x - X) < 4 && Math.abs(m.position.z - Z) < 4), 'siguen dentro del corral')
// A 1-block step can still be jumped: lowering the wall, the zombie gets out to the player.
b.chat('/gamemode creative'); await sleep(300)
b.chat(`/fill ${X - 4} ${Y + 1} ${Z - 4} ${X + 4} ${Y + 1} ${Z + 4} air`); await sleep(300)
b.chat('/gamemode survival')
const zombie = mobs.find(m => m.name === 'zombie')
let out = false
for (let t = 0; t < 200 && !out; t++) { out = Math.abs(zombie.position.x - X) > 4.5 || Math.abs(zombie.position.z - Z) > 4.5; await sleep(50) }
check(out, `el zombi salta el escalón de 1 bloque (${zombie.position})`)
b.chat('/gamemode creative'); await sleep(300)

// Pathfinding: a zombie behind a long 2-block wall walks around it to the player.
{
  const [X2, Z2] = [X + 40, Z]
  b.chat(`/tp ${X2} ${Y + 2} ${Z2}`); await sleep(2500)
  b.chat(`/fill ${X2 - 12} ${Y - 1} ${Z2 - 12} ${X2 + 12} ${Y - 1} ${Z2 + 12} stone`)
  b.chat(`/fill ${X2 - 12} ${Y} ${Z2 - 12} ${X2 + 12} ${Y + 3} ${Z2 + 12} air`)
  b.chat(`/fill ${X2 - 12} ${Y + 4} ${Z2 - 12} ${X2 + 12} ${Y + 4} ${Z2 + 12} stone`)
  await sleep(800)
  b.chat(`/tp ${X2 - 1} ${Y} ${Z2}`); await sleep(800)
  b.chat('/summon zombie'); await sleep(500)
  b.chat(`/tp ${X2 + 5} ${Y} ${Z2}`); await sleep(300)
  b.chat('/gamemode survival'); await sleep(1500) // it sees the player...
  b.chat(`/fill ${X2 + 2} ${Y} ${Z2 - 7} ${X2 + 2} ${Y + 1} ${Z2 + 7} cobblestone`) // ...then a wall goes up
  const z = Object.values(b.entities).find(e => e.name === 'zombie' && Math.abs(e.position.x - X2) < 10 && Math.abs(e.position.z - Z2) < 10)
  check(!!z, 'zombi detrás de la muralla')
  let reached = false, jumps = 0
  for (let t = 0; t < 400 && z && !reached; t++) { // 20 s
    reached = z.position.distanceTo(b.entity.position) < 2.5
    if (z.position.y > Y + 0.3) jumps++
    await sleep(50)
  }
  check(reached, `el zombi rodea la muralla y llega al jugador (${z?.position})`)
  check(jumps < 20, `sin saltar contra la muralla (${jumps} muestras en el aire)`)
  b.chat('/gamemode creative'); await sleep(300)
}
// Pathfinding: the player on a 4-block tower; the only way up is a ladder.
{
  const [X3, Z3] = [X + 80, Z]
  b.chat(`/tp ${X3} ${Y + 2} ${Z3}`); await sleep(2500)
  b.chat(`/fill ${X3 - 10} ${Y - 1} ${Z3 - 10} ${X3 + 10} ${Y - 1} ${Z3 + 10} stone`)
  b.chat(`/fill ${X3 - 10} ${Y} ${Z3 - 10} ${X3 + 10} ${Y + 8} ${Z3 + 10} air`)
  b.chat(`/fill ${X3 + 3} ${Y} ${Z3 - 2} ${X3 + 6} ${Y + 3} ${Z3 + 2} stone`)
  b.chat(`/fill ${X3 + 2} ${Y} ${Z3} ${X3 + 2} ${Y + 3} ${Z3} ladder[facing=west]`)
  await sleep(800)
  b.chat(`/tp ${X3 - 4} ${Y} ${Z3}`); await sleep(800)
  b.chat('/summon zombie'); await sleep(500)
  b.chat(`/tp ${X3 + 5} ${Y + 4} ${Z3}`); await sleep(300)
  b.chat('/gamemode survival')
  const z = Object.values(b.entities).find(e => e.name === 'zombie' && Math.abs(e.position.x - X3) < 10 && Math.abs(e.position.z - Z3) < 10)
  check(!!z, 'zombi al pie de la torre')
  check(await until(() => z && z.position.y >= Y + 3.5, 25000), `sube por la escalera (${z?.position})`)
  b.chat('/gamemode creative'); await sleep(300)
}
b.quit()
console.log(failed ? `\n${failed} fallaron` : '\nTodo OK')
process.exit(failed ? 1 : 0)
