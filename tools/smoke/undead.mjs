// Undead AI: they look for shade before sunrise; skeletons shoot only what they see.
// Usage: node undead.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failed = 0
const check = (ok, what) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${what}`); if (!ok) failed++ }
const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end; await sleep(100)) if (f()) return true; return false }
const b = mineflayer.createBot({ host, port, username: 'Undead', version: '1.20.4', auth: 'offline' })
b._client.on('registry_data', p => b.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
await once(b, 'spawn'); await sleep(1500)
const [X, Y, Z] = [-7000, 150, 7000]
const near = (name, r = 20) => Object.values(b.entities).filter(e => e.name === name && e.position.distanceTo(b.entity.position) < r)
const platform = async (x, z) => {
  b.chat('/gamemode creative'); b.chat(`/tp ${x} ${Y + 2} ${z}`); await sleep(2500)
  b.chat(`/fill ${x - 14} ${Y - 1} ${z - 14} ${x + 14} ${Y - 1} ${z + 14} stone`)
  b.chat(`/fill ${x - 14} ${Y} ${z - 14} ${x + 14} ${Y + 6} ${z + 14} air`); await sleep(800)
}

// 1. Shortly before sunrise a zombie in the open goes under the only roof around.
await platform(X, Z)
b.chat(`/fill ${X + 6} ${Y + 3} ${Z - 2} ${X + 10} ${Y + 3} ${Z + 2} oak_planks`)
b.chat('/time set 22600'); await sleep(500)
b.chat(`/tp ${X} ${Y} ${Z}`); await sleep(800)
b.chat('/summon zombie'); await sleep(300)
b.chat(`/tp ${X - 8} ${Y} ${Z - 8}`)
const zombie = near('zombie')[0]
check(!!zombie, 'zombi al aire libre antes del amanecer')
const roofed = () => zombie && zombie.position.x > X + 6 && zombie.position.x < X + 11 && Math.abs(zombie.position.z - Z) < 3
check(await until(roofed, 20000), `busca la sombra del techo antes de que salga el sol (${zombie?.position})`)
b.chat('/time set 1000'); await sleep(4000)
check(roofed(), `de día no sale de la sombra (${zombie?.position})`)
check(zombie && b.entities[zombie.id] && !(zombie.metadata?.[0] & 1), 'no se quema')

// 2. Skeleton at night: shoots the player it sees and draws the bow first.
await platform(X + 60, Z)
b.chat('/time set 18000')
b.chat(`/tp ${X + 60} ${Y} ${Z}`); await sleep(800)
b.chat('/summon skeleton'); await sleep(300)
b.chat(`/tp ${X + 60 + 9} ${Y} ${Z}`); await sleep(300)
const skel = near('skeleton')[0]
let drew = false
b._client.on('entity_metadata', p => { if (p.entityId === skel?.id && p.metadata.some(m => m.key === 8 && (m.value & 1))) drew = true })
let arrows = 0
b.on('entityMoved', () => {})
b._client.on('spawn_entity', p => { if (b.registry.entities[p.type]?.name === 'arrow') arrows++ })
b.chat('/gamemode survival')
check(await until(() => arrows > 0, 8000), 'el esqueleto dispara flechas al jugador')
check(drew, 'tensa el arco antes de disparar')
check(await until(() => b.health < 20, 6000), `las flechas hieren al jugador (vida ${b.health})`)
// Behind a wall (no line of sight): it does not waste arrows against the wall.
b.chat('/gamemode creative'); await sleep(300)
b.chat('/killmobs skeleton 50'); await sleep(300)
await platform(X + 120, Z)
b.chat(`/tp ${X + 120} ${Y} ${Z}`); await sleep(800)
b.chat('/summon skeleton'); await sleep(300)
b.chat(`/tp ${X + 128} ${Y} ${Z}`); await sleep(300)
const [Bx, Bz] = [X + 120, Z]
for (const [x1, z1, x2, z2] of [[Bx - 3, Bz - 3, Bx + 3, Bz - 3], [Bx - 3, Bz + 3, Bx + 3, Bz + 3], [Bx - 3, Bz - 3, Bx - 3, Bz + 3], [Bx + 3, Bz - 3, Bx + 3, Bz + 3]]) {
  b.chat(`/fill ${x1} ${Y} ${z1} ${x2} ${Y + 3} ${z2} cobblestone`)
}
b.chat(`/fill ${Bx - 3} ${Y + 3} ${Bz - 3} ${Bx + 3} ${Y + 3} ${Bz + 3} cobblestone`); await sleep(500)
b.chat(`/tp ${X + 128} ${Y} ${Z}`); await sleep(500)
b.chat('/gamemode survival')
arrows = 0
await sleep(5000)
check(arrows === 0, `encerrado sin ver al jugador no dispara (${arrows} flechas)`)
b.chat('/gamemode creative'); await sleep(300)
b.quit()
console.log(failed ? `\n${failed} fallaron` : '\nTodo OK')
process.exit(failed ? 1 : 0)
