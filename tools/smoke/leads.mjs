// Leads: putting an animal on a lead, leading it, tying it to a fence (knot), untying it by
// hitting the knot, letting it go, and the lead snapping when too far away.
// Usage: node leads.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
import { Vec3 } from 'vec3'

const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failures = 0
const check = (ok, msg) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${msg}`); if (!ok) failures++ }
const until = async (fn, ms = 4000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await fn()) return true; await sleep(100) } return false }

const a = mineflayer.createBot({ host, port, username: 'Shepherd', version: '1.20.4', auth: 'offline' })
a._client.on('registry_data', p => a.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
await once(a, 'spawn'); await a.waitForChunksToLoad().catch(() => {})
const held = new Map()
a._client.on('attach_entity', p => held.set(p.entityId, p.vehicleId))
const items = name => Object.values(a.entities).some(e => e.name === 'item' && e.getDroppedItem?.()?.name === name)
a.chat('/gamemode survival'); a.chat('/killmobs all 60')
a.chat('/tp 12000.5 130 12000.5'); await sleep(1500)
a.chat('/fill 11980 99 11980 12020 99 12020 grass_block'); a.chat('/fill 11980 100 11980 12020 106 12020 air'); await sleep(600)
a.chat('/tp 12000.5 100 12000.5'); await sleep(800)
a.chat('/summon cow 12002.5 100 12000.5'); await sleep(500)
const cow = Object.values(a.entities).find(e => e.name === 'cow')
a.chat('/give lead 2'); await until(() => a.inventory.items().some(i => i.name === 'lead'), 2000)
await a.equip(a.inventory.items().find(i => i.name === 'lead'), 'hand')
await a.lookAt(cow.position.offset(0, 1, 0), true); await a.activateEntity(cow)
check(await until(() => held.get(cow.id) === a.entity.id, 2000), 'la vaca queda atada al jugador')
// Walking away, the cow comes along.
for (let i = 1; i <= 8; i++) { a.chat(`/tp ${12000.5 - i} 100 12000.5`); await sleep(400) }
await sleep(1500)
check(cow.position.distanceTo(a.entity.position) < 7 && cow.position.x < 12000, `la vaca sigue al jugador (a ${cow.position.distanceTo(a.entity.position).toFixed(1)})`)
// Tied to a fence.
a.chat(`/setblock 11990 100 12003 oak_fence`); await sleep(300)
a.chat('/tp 11990.5 100 12001.5'); await sleep(1500)
await a.activateBlock(a.blockAt(new Vec3(11990, 100, 12003)))
const knot = () => Object.values(a.entities).find(e => e.name === 'leash_knot')
check(await until(() => knot() != null, 2000), 'atada a una valla aparece el nudo')
check(await until(() => held.get(cow.id) === knot()?.id, 2000), 'y la vaca queda atada al nudo')
for (let i = 1; i <= 16; i++) { a.chat(`/tp ${11990.5 + i} 100 12001.5`); await sleep(150) }
await sleep(2000)
check(cow.position.distanceTo(new Vec3(11990.5, 100, 12003.5)) < 8, 'se queda junto a la valla aunque el jugador se vaya')
// Hitting the knot unties it.
a.chat('/tp 11990.5 100 12001.5'); await sleep(800)
a.attack(knot()); await sleep(800)
check(held.get(cow.id) === -1 && knot() == null, 'al golpear el nudo se desata')
check(await until(() => items('lead'), 2000), 'y suelta la cuerda')
// Snapping.
await a.equip(a.inventory.items().find(i => i.name === 'lead'), 'hand')
a.chat(`/tp ${cow.position.x.toFixed(1)} 100 ${(cow.position.z - 2).toFixed(1)}`); await sleep(500)
await a.lookAt(cow.position.offset(0, 1, 0), true); await a.activateEntity(cow)
check(await until(() => held.get(cow.id) === a.entity.id, 2000), 'se vuelve a atar')
a.chat(`/tp ${(cow.position.x + 20).toFixed(1)} 100 ${cow.position.z.toFixed(1)}`)
check(await until(() => held.get(cow.id) === -1, 3000), 'si el jugador se aleja demasiado la cuerda se rompe')
a.quit()
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
