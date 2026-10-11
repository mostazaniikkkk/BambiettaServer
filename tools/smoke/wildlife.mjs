// Animals and utility mobs: iron golem (fights monsters, can be built), horse (tamed by
// riding, saddle, driving, inventory), llama (spits), fish and axolotls (water, buckets),
// pufferfish, rabbit (runs), fox (hunts), turtle (lays eggs at home), goat (milk), frog
// (eats small slimes), polar bear (fights back), parrot (tamed, rides on the shoulder),
// dolphin (Dolphin's Grace).
// Usage: node wildlife.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
import { Vec3 } from 'vec3'

const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failures = 0
const check = (ok, msg) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${msg}`); if (!ok) failures++ }
const until = async (fn, ms = 4000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await fn()) return true; await sleep(100) } return false }
const step = async (name, fn) => { try { await fn() } catch (e) { check(false, `${name}: ${e.message}`) } }

const a = mineflayer.createBot({ host, port, username: 'Ranger', version: '1.20.4', auth: 'offline' })
await once(a, 'spawn'); await a.waitForChunksToLoad().catch(() => {})
const effects = new Set()
a._client.on('entity_effect', p => { if (p.entityId === a.entity.id) effects.add(a.registry.effects[p.effectId]?.name?.toLowerCase().replace(/_/g, '')) })
// mineflayer does not always clear bot.vehicle: follow the passengers packets.
let riding = null
a._client.on('set_passengers', p => { if (p.passengers.includes(a.entity.id)) riding = p.entityId; else if (riding === p.entityId) riding = null })
const mobs = name => Object.values(a.entities).filter(e => e.name === name)
const near = name => mobs(name).sort((x, y) => x.position.distanceTo(a.entity.position) - y.position.distanceTo(a.entity.position))[0]
const B = (x, y, z) => a.blockAt(new Vec3(x, y, z))
const arena = async (x, z, floor = 'stone') => {
  a.chat('/gamemode creative'); a.chat('/killmobs all 60')
  a.chat(`/tp ${x}.5 130 ${z}.5`); await sleep(1500)
  a.chat(`/fill ${x - 12} 99 ${z - 12} ${x + 12} 99 ${z + 12} ${floor}`); a.chat(`/fill ${x - 12} 100 ${z - 12} ${x + 12} 106 ${z + 12} air`); await sleep(600)
  a.chat(`/tp ${x}.5 100 ${z}.5`); a.chat('/heal'); await sleep(800)
  effects.clear()
}
const pool = async (x, z) => {
  a.chat(`/fill ${x - 6} 95 ${z - 6} ${x + 6} 99 ${z + 6} water`); await sleep(400)
  a.chat(`/fill ${x - 7} 94 ${z - 7} ${x + 7} 94 ${z + 7} stone`); await sleep(300)
}
const give = async (item, n = 1) => {
  const name = item.split('{')[0]
  a.chat(`/give ${item} ${n}`)
  await until(() => a.inventory.items().some(i => i.name === name), 2000)
  await a.equip(a.inventory.items().find(i => i.name === name), 'hand')
}
const use = async e => { await a.lookAt(e.position.offset(0, 0.5, 0), true); await a.activateEntity(e); await sleep(250) }
a.chat('/gamerule doDaylightCycle false'); a.chat('/time set 1000')

await step('gólem de hierro', async () => {
  await arena(7000, 7000)
  a.chat('/summon iron_golem 7004.5 100 7000.5'); a.chat('/summon zombie 7008.5 100 7000.5'); await sleep(500)
  const z = near('zombie')
  check(await until(() => !a.entities[z?.id], 15000), 'el gólem mata al zombi')
  a.chat('/killmobs all 60')
  // Built by hand: a T of iron blocks and a pumpkin on top.
  a.chat('/setblock 7000 100 7004 iron_block'); a.chat('/setblock 7000 101 7004 iron_block')
  a.chat('/setblock 6999 101 7004 iron_block'); a.chat('/setblock 7001 101 7004 iron_block'); await sleep(400)
  a.chat('/tp 7000.5 100 7001.5'); await sleep(400)
  await give('carved_pumpkin')
  await a.placeBlock(B(7000, 101, 7004), new Vec3(0, 1, 0)).catch(() => {})
  check(await until(() => mobs('iron_golem').length === 1, 3000), 'una T de hierro con una calabaza forma un gólem')
  check(B(7000, 101, 7004)?.name === 'air', 'y los bloques desaparecen')
})

await step('caballo', async () => {
  await arena(7040, 7000, 'grass_block')
  a.chat('/summon horse 7043.5 100 7000.5'); await sleep(800)
  const h = near('horse')
  a.chat('/gamemode survival'); await sleep(200)
  let tamed = false
  for (let i = 0; i < 40 && !tamed; i++) {
    if (riding == null) {
      a.chat(`/tp ${(h.position.x - 1.5).toFixed(1)} ${h.position.y.toFixed(1)} ${h.position.z.toFixed(1)}`); await sleep(300)
      await use(h)
    }
    await sleep(700)
    tamed = (h.metadata?.[17] & 0x02) !== 0
  }
  check(tamed, 'montándolo varias veces se doma')
  if (riding != null) { a._client.write('steer_vehicle', { sideways: 0, forward: 0, jump: 0x02 }); await sleep(600) }
  check(riding == null, 'se baja con shift')
  await give('saddle')
  await use(h)
  check(await until(() => (h.metadata?.[17] & 0x04) !== 0, 2000), 'se le pone la montura')
  await use(h)
  check(await until(() => riding === h.id, 2000), 'se monta')
  // The client drives it (vehicle_move).
  const from = h.position.clone()
  for (let i = 1; i <= 10; i++) {
    a._client.write('vehicle_move', { x: from.x + i * 0.5, y: from.y, z: from.z, yaw: -90, pitch: 0 }); await sleep(50)
  }
  await sleep(500)
  const watcher = mineflayer.createBot({ host, port, username: 'Watcher', version: '1.20.4', auth: 'offline' })
  await once(watcher, 'spawn'); watcher.chat(`/tp ${from.x.toFixed(1)} 100 ${(from.z + 4).toFixed(1)}`); await sleep(2000)
  const seenHorse = Object.values(watcher.entities).find(e => e.name === 'horse')
  check(seenHorse && seenHorse.position.x > from.x + 3, `al conducirlo el caballo avanza para los demás (${from.x.toFixed(1)} -> ${seenHorse?.position.x.toFixed(1)})`)
  watcher.quit()
  let horseWindow = null
  a._client.once('open_horse_window', p => { horseWindow = p })
  a._client.write('entity_action', { entityId: a.entity.id, actionId: 7, jumpBoost: 0 })
  check(await until(() => horseWindow != null, 2000), `se abre su inventario (${horseWindow?.nbSlots} ranuras)`)
  a._client.write('steer_vehicle', { sideways: 0, forward: 0, jump: 0x02 }); await sleep(500)
  a.chat('/gamemode creative')
})

await step('llama', async () => {
  await arena(7080, 7000, 'grass_block')
  a.chat('/summon llama 7084.5 100 7000.5'); await sleep(500)
  const l = near('llama')
  a.chat('/gamemode survival'); await sleep(200)
  await a.lookAt(l.position.offset(0, 1, 0), true); a.attack(l)
  check(await until(() => mobs('llama_spit').length > 0 || a.health < 20, 5000), 'la llama escupe a quien la golpea')
  a.chat('/gamemode creative')
})

await step('peces y cubos', async () => {
  await arena(7120, 7000)
  await pool(7120, 7000)
  a.chat('/summon cod 7122.5 97 7000.5'); await sleep(2500)
  const c = near('cod')
  check(c && c.position.y < 99.5, `el bacalao se queda en el agua (y ${c?.position.y.toFixed(1)})`)
  await give('water_bucket')
  a.chat(`/tp ${c.position.x.toFixed(1)} ${(c.position.y + 1).toFixed(1)} ${(c.position.z + 1.5).toFixed(1)}`); await sleep(400)
  await a.lookAt(c.position, true); await a.activateEntity(c); await sleep(400)
  check(a.inventory.items().some(i => i.name === 'cod_bucket'), 'con un cubo de agua se recoge el pez')
  await a.equip(a.inventory.items().find(i => i.name === 'cod_bucket'), 'hand')
  await a.lookAt(new Vec3(7120.5, 99, 7003.5), true); a.activateItem(); await sleep(800)
  check(await until(() => mobs('cod').length > 0, 2000), 'al vaciar el cubo el pez vuelve al agua')
  // (Axolotls hunt fish: it comes after the cod.)
  a.chat('/summon axolotl 7118.5 97 7000.5'); await sleep(800)
  const ax = near('axolotl')
  await give('water_bucket')
  a.chat(`/tp ${ax.position.x.toFixed(1)} ${(ax.position.y + 1).toFixed(1)} ${(ax.position.z + 1.5).toFixed(1)}`); await sleep(400)
  await a.lookAt(ax.position, true); await a.activateEntity(ax); await sleep(400)
  check(a.inventory.items().some(i => i.name === 'axolotl_bucket'), 'el ajolote también va al cubo')
})

await step('pez globo', async () => {
  await arena(7160, 7000)
  await pool(7160, 7000)
  a.chat('/summon pufferfish 7160.5 98 7001.5'); await sleep(300)
  a.chat('/gamemode survival'); a.chat('/tp 7160.5 98 7000.5'); await sleep(200)
  const p = near('pufferfish')
  check(await until(() => p?.metadata?.[17] === 2, 4000), `se infla con el jugador cerca (estado ${p?.metadata?.[17]})`)
  check(await until(() => {
    a.chat(`/tp ${p.position.x.toFixed(2)} ${p.position.y.toFixed(2)} ${p.position.z.toFixed(2)}`)
    return effects.has('poison')
  }, 6000), 'y envenena al tocarlo')
  a.chat('/gamemode creative'); a.chat('/effect clear')
})

await step('conejo y zorro', async () => {
  await arena(7200, 7000, 'grass_block')
  a.chat('/gamemode survival')
  a.chat('/summon rabbit 7202.5 100 7000.5'); await sleep(500)
  const r = near('rabbit')
  const d0 = r.position.distanceTo(a.entity.position)
  check(await until(() => r.position.distanceTo(a.entity.position) > d0 + 2.5, 6000), `el conejo huye (${d0.toFixed(1)} -> ${r.position.distanceTo(a.entity.position).toFixed(1)})`)
  a.chat('/gamemode creative'); a.chat('/time set 14000')
  a.chat('/summon fox 7208.5 100 7008.5'); a.chat('/summon chicken 7212.5 100 7008.5'); await sleep(300)
  check(await until(() => mobs('chicken').length === 0, 20000), 'el zorro caza la gallina')
  a.chat('/time set 1000')
})

await step('tortuga', async () => {
  await arena(7240, 7000, 'sand')
  a.chat('/summon turtle 7240.5 100 7004.5'); a.chat('/summon turtle 7241.5 100 7004.5'); await sleep(500)
  await give('seagrass', 4)
  for (const t of mobs('turtle')) await use(t)
  check(await until(() => {
    for (let dx = -6; dx <= 6; dx++) for (let dz = -2; dz <= 10; dz++) if (B(7240 + dx, 100, 7000 + dz)?.name === 'turtle_egg') return true
    return false
  }, 30000), 'y los pone en la arena de su playa')
})

await step('cabra', async () => {
  await arena(7280, 7000, 'grass_block')
  a.chat('/summon goat 7282.5 100 7000.5'); await sleep(500)
  await give('bucket')
  await use(near('goat'))
  check(a.inventory.items().some(i => i.name === 'milk_bucket'), 'la cabra se ordeña')
})

await step('rana', async () => {
  await arena(7320, 7000, 'grass_block')
  a.chat('/summon frog 7320.5 100 7003.5'); a.chat('/summon slime 7323.5 100 7003.5 1'); await sleep(300)
  check(await until(() => mobs('slime').length === 0, 12000), 'la rana se come el slime pequeño')
  check(await until(() => Object.values(a.entities).some(e => e.name === 'item' && e.getDroppedItem?.()?.name === 'slime_ball'), 2000), 'y deja una bola de slime')
})

await step('oso polar', async () => {
  await arena(7360, 7000, 'snow_block')
  a.chat('/summon polar_bear 7362.5 100 7000.5'); await sleep(500)
  a.chat('/gamemode survival'); a.chat('/heal'); await sleep(200)
  const b = near('polar_bear')
  await a.lookAt(b.position.offset(0, 1, 0), true); a.attack(b)
  check(await until(() => a.health < 20, 6000), `el oso polar devuelve el golpe (vida ${a.health})`)
  a.chat('/gamemode creative'); a.chat('/heal')
})

await step('loro', async () => {
  await arena(7400, 7000, 'grass_block')
  a.chat('/summon parrot 7401.5 100 7000.5'); await sleep(800)
  const p = near('parrot')
  await give('wheat_seeds', 64)
  for (let i = 0; i < 30 && !((p.metadata?.[17] & 0x04) !== 0); i++) { await use(p); await sleep(150) }
  check((p.metadata?.[17] & 0x04) !== 0, 'el loro se doma con semillas')
  await use(p) // stands up
  await a.equip(a.inventory.items().find(i => i.name !== 'wheat_seeds') ?? null, 'hand').catch(() => {})
  a.chat(`/tp ${p.position.x.toFixed(1)} 100 ${p.position.z.toFixed(1)}`)
  const onShoulder = () => { const v = a.entity.metadata?.[19]; return v && typeof v === 'object' && Object.keys(v.value ?? v).length > 0 }
  check(await until(onShoulder, 15000), 'se sube al hombro de su dueño')
})

await step('delfín', async () => {
  await arena(7440, 7000)
  await pool(7440, 7000)
  a.chat('/summon dolphin 7441.5 97 7000.5'); a.chat('/tp 7440.5 97 7000.5'); await sleep(200)
  check(await until(() => effects.has('dolphinsgrace'), 6000), 'nadar con un delfín da Gracia del delfín')
})

a.chat('/gamemode creative'); a.chat('/killmobs all 60'); a.chat('/gamerule doDaylightCycle true')
await sleep(300)
a.quit()
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
