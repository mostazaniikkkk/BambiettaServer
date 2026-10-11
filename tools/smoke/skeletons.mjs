// Skeleton tactics on hard difficulty: backs off when the player closes in, goes round a
// shield, and takes cover from a player holding a bow (stepping out only to shoot).
// Requires "difficulty": 3. Usage: node skeletons.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failed = 0
const check = (ok, what) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${what}`); if (!ok) failed++ }
const b = mineflayer.createBot({ host, port, username: 'Archer', version: '1.20.4', auth: 'offline' })
await once(b, 'spawn'); await sleep(1500)
const Y = 150
const skeleton = () => Object.values(b.entities).filter(e => e.name === 'skeleton').sort((p, q) => p.position.distanceTo(b.entity.position) - q.position.distanceTo(b.entity.position))[0]
const arena = async (X, Z) => {
  b.chat('/gamemode creative'); b.chat('/killmobs skeleton 200'); b.chat('/clear')
  b.chat(`/tp ${X} ${Y + 3} ${Z}`); await sleep(2500)
  b.chat(`/fill ${X - 16} ${Y - 1} ${Z - 16} ${X + 16} ${Y - 1} ${Z + 16} stone`)
  b.chat(`/fill ${X - 16} ${Y} ${Z - 16} ${X + 16} ${Y + 5} ${Z + 16} air`)
  b.chat(`/fill ${X - 16} ${Y + 6} ${Z - 16} ${X + 16} ${Y + 6} ${Z + 16} stone`) // roof: no sun
  await sleep(800)
}
// Slab test: does the segment a-b cross the box [lo, hi]?
const segmentHitsBox = (a, b2, lo, hi) => {
  let t0 = 0, t1 = 1
  const A = [a.x, a.y, a.z], D = [b2.x - a.x, b2.y - a.y, b2.z - a.z]
  for (let i = 0; i < 3; i++) {
    if (Math.abs(D[i]) < 1e-9) { if (A[i] < lo[i] || A[i] > hi[i]) return false; continue }
    let u = (lo[i] - A[i]) / D[i], v = (hi[i] - A[i]) / D[i]
    if (u > v) [u, v] = [v, u]
    t0 = Math.max(t0, u); t1 = Math.min(t1, v)
    if (t0 > t1) return false
  }
  return true
}
const summonAt = async (x, z) => { b.chat(`/tp ${x} ${Y} ${z}`); await sleep(600); b.chat('/summon skeleton'); await sleep(500) }
b.chat('/time set 18000')

// 1. Closing in: it backs off and keeps its distance.
{
  const [X, Z] = [12000, 12000]
  await arena(X, Z)
  await summonAt(X, Z)
  b.chat(`/tp ${X + 10} ${Y} ${Z}`); await sleep(300)
  b.chat('/gamemode survival'); b.chat('/effect resistance 120 5'); await sleep(1500)
  const s = skeleton()
  // The player walks straight at it.
  for (let i = 0; i < 12; i++) {
    const p = s.position
    const me = b.entity.position
    const dx = p.x - me.x, dz = p.z - me.z, len = Math.hypot(dx, dz)
    b.chat(`/tp ${(me.x + dx / len * 0.8).toFixed(2)} ${Y} ${(me.z + dz / len * 0.8).toFixed(2)}`)
    await sleep(250)
  }
  await sleep(1500)
  const d = s.position.distanceTo(b.entity.position)
  check(d > 5, `al acercarse el jugador retrocede (${d.toFixed(1)} bloques)`)
}

// 2. Shield: it moves round to where the shield does not cover.
{
  const [X, Z] = [12100, 12000]
  await arena(X, Z)
  await summonAt(X, Z)
  b.chat(`/tp ${X + 9} ${Y} ${Z}`); await sleep(300)
  b.chat('/give shield'); await sleep(500)
  const shield = b.inventory.items().find(i => i.name === 'shield')
  if (shield) await b.equip(shield, 'off-hand')
  b.chat('/gamemode survival'); b.chat('/effect resistance 120 5'); await sleep(500)
  const looking = b.entity.position.offset(-9, 1, 0) // looks at where the skeleton started
  let behind = false
  for (let i = 0; i < 60 && !behind; i++) {
    await b.lookAt(looking, true)
    if (i % 10 === 0) b.activateItem(true) // shield up (off hand)
    const s = skeleton()
    if (s) {
      const me = b.entity.position
      const lx = looking.x - me.x, lz = looking.z - me.z, ll = Math.hypot(lx, lz)
      const sx = s.position.x - me.x, sz = s.position.z - me.z, sl = Math.hypot(sx, sz)
      behind = (lx * sx + lz * sz) / (ll * sl) < -0.1
    }
    await sleep(250)
  }
  b.deactivateItem()
  check(behind, `con escudo, lo rodea hasta quedar detrás del jugador (${skeleton()?.position})`)
}

// 3. Bow: it hides behind a pillar and only steps out to shoot.
{
  const [X, Z] = [12200, 12000]
  await arena(X, Z)
  b.chat(`/fill ${X + 3} ${Y} ${Z - 1} ${X + 3} ${Y + 5} ${Z + 1} stone`) // pillar between both
  await summonAt(X, Z + 5)
  b.chat(`/tp ${X + 12} ${Y} ${Z}`); await sleep(300)
  b.chat('/give bow'); await sleep(500)
  const bow = b.inventory.items().find(i => i.name === 'bow')
  if (bow) await b.equip(bow, 'hand')
  b.chat('/gamemode survival'); b.chat('/effect resistance 120 5'); await sleep(500)
  let arrows = 0
  b._client.on('spawn_entity', p => { if (b.registry.entities[p.type]?.name === 'arrow') arrows++ })
  let hidden = 0, samples = 0
  await sleep(5000) // time to find cover
  for (let i = 0; i < 60; i++) {
    const s = skeleton()
    if (s) {
      samples++
      // Hidden: the segment from the player's eyes to the skeleton's head crosses the pillar.
      const eye = b.entity.position.offset(0, 1.62, 0)
      const head = s.position.offset(0, 1.7, 0)
      if (segmentHitsBox(eye, head, [X + 3, Y, Z - 1], [X + 4, Y + 6, Z + 2])) hidden++
    }
    await sleep(250)
  }
  check(samples > 0 && hidden / samples > 0.5, `ante un arco se cubre tras el pilar (${hidden}/${samples} muestras oculto)`)
  check(arrows > 0, `sale de la cobertura para disparar (${arrows} flechas)`)
}
b.chat('/gamemode creative')
b.quit()
console.log(failed ? `\n${failed} fallaron` : '\nTodo OK')
process.exit(failed ? 1 : 0)
