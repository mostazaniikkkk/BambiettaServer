// Mob perception and roaming: land mobs get out of the water, monsters only notice a player
// they can see, spiders are calm in the light unless hit, skeletons chase a player that walks
// away, and monsters patrol when nobody is around.
// Usage: node perception.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failed = 0
const check = (ok, what) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${what}`); if (!ok) failed++ }
const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end; await sleep(100)) if (f()) return true; return false }
const b = mineflayer.createBot({ host, port, username: 'Watcher', version: '1.20.4', auth: 'offline' })
await once(b, 'spawn'); await sleep(1500)
const Y = 150
const angry = new Set()
b._client.on('entity_metadata', p => { for (const m of p.metadata) if (m.key === 15) (m.value & 4 ? angry.add(p.entityId) : angry.delete(p.entityId)) })
const find = (name, x, z, r = 6) => Object.values(b.entities).find(e => e.name === name && Math.hypot(e.position.x - x, e.position.z - z) < r)
const arena = async (X, Z, roof) => {
  b.chat('/gamemode creative'); b.chat('/killmobs all 80')
  b.chat(`/tp ${X} ${Y + 3} ${Z}`); await sleep(2500)
  b.chat(`/fill ${X - 20} ${Y - 1} ${Z - 20} ${X + 20} ${Y - 1} ${Z + 20} stone`)
  b.chat(`/fill ${X - 20} ${Y} ${Z - 20} ${X + 20} ${Y + 5} ${Z + 20} air`)
  if (roof) b.chat(`/fill ${X - 20} ${Y + 6} ${Z - 20} ${X + 20} ${Y + 6} ${Z + 20} stone`)
  await sleep(800)
}
const summon = async (name, x, z) => { b.chat(`/tp ${x} ${Y} ${z}`); await sleep(500); b.chat(`/summon ${name}`); await sleep(400) }

// 1. A zombie in a pond swims to the bank and climbs out.
{
  const [X, Z] = [20000, 20000]
  b.chat('/time set 18000')
  await arena(X, Z, true)
  b.chat(`/fill ${X - 5} ${Y - 5} ${Z - 5} ${X + 5} ${Y - 1} ${Z + 5} stone`) // a basin (the arena floats)
  b.chat(`/fill ${X - 3} ${Y - 3} ${Z - 3} ${X + 3} ${Y - 1} ${Z + 3} water`); await sleep(500)
  await summon('zombie', X, Z)
  b.chat(`/tp ${X + 15} ${Y + 4} ${Z}`) // creative: not a target
  const z = find('zombie', X, Z)
  const out = () => z && (Math.abs(z.position.x - X) > 3.6 || Math.abs(z.position.z - Z) > 3.6) && z.position.y >= Y - 0.1
  check(await until(out, 20000), `el zombi sale del agua a tierra firme (${z?.position})`)
}

// 1b. Same with a bank one block above the water (like a lake shore).
{
  const [X, Z] = [20050, 20000]
  await arena(X, Z, true)
  b.chat(`/fill ${X - 5} ${Y - 5} ${Z - 5} ${X + 5} ${Y - 1} ${Z + 5} stone`) // a basin (the arena floats)
  b.chat(`/fill ${X - 3} ${Y - 4} ${Z - 3} ${X + 3} ${Y - 2} ${Z + 3} water`)
  b.chat(`/fill ${X - 3} ${Y - 1} ${Z - 3} ${X + 3} ${Y - 1} ${Z + 3} air`); await sleep(500)
  for (const name of ['zombie', 'skeleton', 'creeper']) {
    await summon(name, X, Z)
  }
  b.chat(`/tp ${X + 15} ${Y + 4} ${Z}`)
  const mobs = ['zombie', 'skeleton', 'creeper'].map(n => find(n, X, Z, 5)).filter(Boolean)
  const out = (m) => (Math.abs(m.position.x - X) > 3.6 || Math.abs(m.position.z - Z) > 3.6) && m.position.y >= Y - 0.1
  check(mobs.length === 3 && await until(() => mobs.every(out), 25000), `zombi, esqueleto y creeper salen por una orilla de un bloque (${mobs.map(m => m.position.y.toFixed(1)).join(', ')})`)
}

// 2. It only notices a player it can see.
{
  const [X, Z] = [20100, 20000]
  await arena(X, Z, true)
  await summon('zombie', X, Z)
  b.chat(`/fill ${X + 2} ${Y} ${Z - 6} ${X + 2} ${Y + 5} ${Z + 6} stone`) // a wall between the two
  b.chat(`/tp ${X + 6} ${Y} ${Z}`); await sleep(300)
  b.chat('/gamemode survival')
  const z = find('zombie', X, Z)
  await sleep(4000)
  check(z && !angry.has(z.id), 'detrás de un muro no detecta al jugador')
  b.chat(`/fill ${X + 2} ${Y} ${Z - 6} ${X + 2} ${Y + 5} ${Z + 6} air`)
  check(await until(() => angry.has(z?.id), 4000), 'al quitar el muro lo ve y lo ataca')
}

// 3. Spiders: calm in daylight, they fight back when hit; in the dark they hunt.
{
  const [X, Z] = [20200, 20000]
  b.chat('/time set 6000')
  await arena(X, Z, false)
  await summon('spider', X, Z)
  b.chat(`/tp ${X + 5} ${Y} ${Z}`); await sleep(300)
  b.chat('/gamemode survival'); b.chat('/effect resistance 60 5')
  const s = find('spider', X, Z)
  await sleep(4000)
  check(s && !angry.has(s.id), 'de día la araña no ataca')
  b.chat(`/tp ${s.position.x.toFixed(1)} ${Y} ${(s.position.z + 2).toFixed(1)}`); await sleep(400)
  await b.lookAt(s.position.offset(0, 0.4, 0), true); b.attack(s)
  check(await until(() => angry.has(s.id), 3000), 'si le pegas se defiende')
  b.chat('/gamemode creative'); b.chat('/killmobs spider 30'); b.chat('/time set 18000'); await sleep(500)
  await summon('spider', X, Z)
  b.chat(`/tp ${X + 6} ${Y} ${Z}`); await sleep(300)
  b.chat('/gamemode survival')
  const s2 = find('spider', X, Z)
  check(await until(() => angry.has(s2?.id), 5000), 'de noche la araña caza')
}

// 4. A skeleton chases a player that walks away.
{
  const [X, Z] = [20300, 20000]
  b.chat('/time set 18000')
  await arena(X, Z, true)
  await summon('skeleton', X - 12, Z)
  b.chat(`/tp ${X - 2} ${Y} ${Z}`); await sleep(300)
  b.chat('/gamemode survival'); b.chat('/effect resistance 60 5')
  const s = find('skeleton', X - 12, Z)
  check(await until(() => angry.has(s?.id), 5000), 'el esqueleto ve al jugador')
  b.chat(`/tp ${X + 16} ${Y} ${Z}`); await sleep(500) // 28 blocks away
  const d0 = s.position.distanceTo(b.entity.position)
  await sleep(6000)
  const d1 = s.position.distanceTo(b.entity.position)
  check(d1 < d0 - 5, `lo persigue cuando se aleja (${d0.toFixed(1)} -> ${d1.toFixed(1)})`)
}

// 5. With nobody around, a skeleton patrols.
{
  const [X, Z] = [20400, 20000]
  await arena(X, Z, true)
  await summon('skeleton', X, Z)
  b.chat('/gamemode creative'); b.chat(`/tp ${X} ${Y + 20} ${Z}`)
  const s = find('skeleton', X, Z, 10)
  const start = s?.position.clone()
  let far = 0
  await until(() => { far = Math.max(far, s.position.distanceTo(start)); return far > 5 }, 25000)
  check(far > 5, `sin nadie a la vista patrulla (${far.toFixed(1)} bloques)`)
}
b.chat('/gamemode creative')
b.quit()
console.log(failed ? `\n${failed} fallaron` : '\nTodo OK')
process.exit(failed ? 1 : 0)
