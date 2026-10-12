// Zombies breaking doors: a zombie breaks the wooden door of the house the player hid in
// (cracks first), several zombies break it faster, and an iron door resists.
// Requires "difficulty": 3 (12 s per zombie, like vanilla). Usage: node doors.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
import { Vec3 } from 'vec3'
const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failed = 0
const check = (ok, what) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${what}`); if (!ok) failed++ }
const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end; await sleep(100)) if (f()) return true; return false }
const b = mineflayer.createBot({ host, port, username: 'Homeowner', version: '1.20.4', auth: 'offline' })
b._client.on('registry_data', p => b.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
await once(b, 'spawn'); await sleep(1500)
const Y = 150
let cracks = 0
b._client.on('block_break_animation', p => { if (p.entityId < 0 && p.destroyStage >= 0 && p.destroyStage < 10) cracks++ })
// A 5x5 room (walls x/z = ±3) with a door in the east wall, a roof, and the floor around.
const house = async (X, Z, door) => {
  b.chat('/gamemode creative'); b.chat('/killmobs all 80')
  b.chat(`/tp ${X} ${Y + 6} ${Z}`); await sleep(2500)
  b.chat(`/fill ${X - 16} ${Y - 1} ${Z - 16} ${X + 16} ${Y - 1} ${Z + 16} stone`)
  b.chat(`/fill ${X - 16} ${Y} ${Z - 16} ${X + 16} ${Y + 5} ${Z + 16} air`)
  b.chat(`/fill ${X - 16} ${Y + 6} ${Z - 16} ${X + 16} ${Y + 6} ${Z + 16} stone`) // night everywhere
  b.chat(`/fill ${X - 3} ${Y} ${Z - 3} ${X + 3} ${Y + 2} ${Z + 3} stone`)
  b.chat(`/fill ${X - 2} ${Y} ${Z - 2} ${X + 2} ${Y + 1} ${Z + 2} air`)
  b.chat(`/setblock ${X + 3} ${Y} ${Z} ${door}[facing=east,half=lower]`)
  b.chat(`/setblock ${X + 3} ${Y + 1} ${Z} ${door}[facing=east,half=upper]`)
  await sleep(800)
}
const doorAt = (X, Z) => b.blockAt(new Vec3(X + 3, Y, Z))?.name
const summonAt = async (x, z, n = 1) => { b.chat(`/tp ${x} ${Y} ${z}`); await sleep(500); for (let i = 0; i < n; i++) { b.chat('/summon zombie'); await sleep(150) } }
// The zombies see the player outside, then it runs into the house and the door closes.
const hide = async (X, Z) => {
  b.chat(`/tp ${X + 6} ${Y} ${Z + 4}`); await sleep(300)
  b.chat('/gamemode survival'); b.chat('/effect resistance 120 5'); await sleep(2000)
  b.chat(`/tp ${X} ${Y} ${Z}`)
}
b.chat('/time set 18000')

// 1. One zombie breaks the wooden door (cracks first).
let single = 0
{
  const [X, Z] = [24000, 24000]
  await house(X, Z, 'oak_door')
  await summonAt(X + 12, Z)
  await hide(X, Z)
  const t0 = Date.now()
  check(await until(() => cracks > 0, 15000), 'el zombi golpea la puerta (aparecen grietas)')
  check(await until(() => doorAt(X, Z) === 'air', 30000), `rompe la puerta de madera (${doorAt(X, Z)})`)
  single = (Date.now() - t0) / 1000
  const z = Object.values(b.entities).find(e => e.name === 'zombie' && e.position.distanceTo(new Vec3(X, Y, Z)) < 20)
  check(await until(() => z && Math.abs(z.position.x - X) < 3 && Math.abs(z.position.z - Z) < 3, 8000), 'entra a la casa')
}

// 2. Three zombies at the same door break it faster.
{
  const [X, Z] = [24100, 24000]
  await house(X, Z, 'oak_door')
  await summonAt(X + 12, Z, 3)
  await hide(X, Z)
  const t0 = Date.now()
  check(await until(() => doorAt(X, Z) === 'air', 30000), `tres zombis rompen la puerta (${doorAt(X, Z)})`)
  const triple = (Date.now() - t0) / 1000
  check(triple < single * 0.75, `entre varios es más rápido (${triple.toFixed(1)} s contra ${single.toFixed(1)} s)`)
}

// 3. An iron door resists.
{
  const [X, Z] = [24200, 24000]
  await house(X, Z, 'iron_door')
  await summonAt(X + 12, Z)
  await hide(X, Z)
  await sleep(20000)
  check(doorAt(X, Z) === 'iron_door', `la puerta de hierro resiste (${doorAt(X, Z)})`)
}
b.chat('/gamemode creative')
b.quit()
console.log(failed ? `\n${failed} fallaron` : '\nTodo OK')
process.exit(failed ? 1 : 0)
