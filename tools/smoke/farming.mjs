// Farming: hoe -> farmland, seeds only on farmland, bone meal, harvest, saplings into trees,
// trampling and crops growing on their own.
// Usage: node farming.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
import { Vec3 } from 'vec3'
const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failed = 0
const check = (ok, what) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${what}`); if (!ok) failed++ }
const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end; await sleep(100)) if (f()) return true; return false }
const b = mineflayer.createBot({ host, port, username: 'Farmer', version: '1.20.4', auth: 'offline' })
b._client.on('registry_data', p => b.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
await once(b, 'spawn'); await sleep(1500)
const [X, Y, Z] = [-12000, 150, -12000]
const at = (dx, dy, dz) => b.blockAt(new Vec3(X + dx, Y + dy, Z + dz))
const hold = async (name, n = 1) => {
  b.chat(`/give ${name} ${n}`)
  await until(() => b.inventory.items().some(i => i.name === name), 3000)
  await b.equip(b.inventory.items().find(i => i.name === name), 'hand')
}
b.chat('/gamemode creative'); b.chat('/time set 1000'); b.chat('/weather clear')
b.chat(`/tp ${X} ${Y + 3} ${Z}`); await sleep(2500)
b.chat(`/fill ${X - 12} ${Y - 1} ${Z - 12} ${X + 12} ${Y - 1} ${Z + 12} grass_block`)
b.chat(`/fill ${X - 12} ${Y} ${Z - 12} ${X + 12} ${Y + 8} ${Z + 12} air`)
await sleep(1000)
b.chat(`/tp ${X + 0.5} ${Y} ${Z - 2.5}`); await sleep(800)
b.chat('/gamemode survival'); await sleep(300)

// 1. Hoe: grass -> farmland.
await hold('iron_hoe')
await b.activateBlock(at(0, -1, 0), new Vec3(0, 1, 0)); await sleep(500)
check(at(0, -1, 0)?.name === 'farmland', `la azada convierte el pasto en tierra de cultivo (${at(0, -1, 0)?.name})`)

// 2. Seeds: on farmland yes, on grass no.
await hold('wheat_seeds', 4)
await b.placeBlock(at(0, -1, 0), new Vec3(0, 1, 0)).catch(() => {}); await sleep(500)
check(at(0, 0, 0)?.name === 'wheat', `las semillas se plantan en la tierra de cultivo (${at(0, 0, 0)?.name})`)
await b.placeBlock(at(2, -1, 0), new Vec3(0, 1, 0)).catch(() => {}); await sleep(500)
check(at(2, 0, 0)?.name !== 'wheat', 'no se plantan sobre el pasto')

// 3. Bone meal until it is ripe, then harvest.
await hold('bone_meal', 16)
for (let i = 0; i < 8 && Number(at(0, 0, 0)?.getProperties?.().age) !== 7; i++) {
  await b.activateBlock(at(0, 0, 0)); await sleep(300)
}
check(Number(at(0, 0, 0)?.getProperties?.().age) === 7, `el polvo de hueso madura el trigo (edad ${at(0, 0, 0)?.getProperties?.().age})`)
await b.dig(at(0, 0, 0)); await sleep(300)
const dropped = Object.values(b.entities).filter(e => e.name === 'item' && e.position.distanceTo(new Vec3(X + 0.5, Y, Z + 0.5)) < 2)
const wheat = dropped.some(e => e.getDroppedItem?.()?.name === 'wheat')
check(dropped.length >= 2 && wheat, `cosechar el trigo maduro suelta trigo y semillas (${dropped.map(e => e.getDroppedItem?.()?.name).join(', ')})`)

// 4. Sapling + bone meal: a tree.
b.chat('/gamemode creative'); b.chat(`/tp ${X - 3.5} ${Y} ${Z + 0.5}`); await sleep(500); b.chat('/gamemode survival'); await sleep(300)
await hold('oak_sapling')
await b.placeBlock(at(-4, -1, 3), new Vec3(0, 1, 0)).catch(() => {}); await sleep(500)
check(at(-4, 0, 3)?.name === 'oak_sapling', 'se planta un brote de roble')
await hold('bone_meal', 16)
for (let i = 0; i < 16 && at(-4, 0, 3)?.name === 'oak_sapling'; i++) {
  await b.activateBlock(at(-4, 0, 3)); await sleep(250)
}
check(at(-4, 0, 3)?.name === 'oak_log' && at(-4, 2, 3)?.name === 'oak_log', `el brote crece como árbol (${at(-4, 0, 3)?.name})`)

// 5. Trampling: falling onto farmland turns it into dirt.
b.chat('/gamemode creative'); await sleep(200)
b.chat(`/setblock ${X + 4} ${Y - 1} ${Z + 4} farmland`); await sleep(300)
b.chat(`/tp ${X + 4.5} ${Y + 2.5} ${Z + 4.5}`); await sleep(200)
b.chat('/gamemode survival')
check(await until(() => at(4, -1, 4)?.name === 'dirt', 4000), `caer sobre la tierra de cultivo la pisotea (${at(4, -1, 4)?.name})`)

// 6. Crops grow on their own (random ticks): a field next to water.
b.chat('/gamemode creative'); await sleep(200)
b.chat(`/fill ${X - 10} ${Y - 1} ${Z + 6} ${X + 10} ${Y - 1} ${Z + 10} farmland[moisture=7]`)
b.chat(`/fill ${X - 10} ${Y - 1} ${Z + 8} ${X + 10} ${Y - 1} ${Z + 8} water`)
b.chat(`/fill ${X - 10} ${Y} ${Z + 6} ${X + 10} ${Y} ${Z + 7} wheat`)
b.chat(`/fill ${X - 10} ${Y} ${Z + 9} ${X + 10} ${Y} ${Z + 10} wheat`)
const grown = () => {
  let n = 0
  for (let dx = -10; dx <= 10; dx++) for (const dz of [6, 7, 9, 10]) if (Number(at(dx, 0, dz)?.getProperties?.().age ?? 0) > 0) n++
  return n
}
check(await until(() => grown() >= 3, 60000), `el trigo crece solo (${grown()} de 84 plantas)`)
b.quit()
console.log(failed ? `\n${failed} fallaron` : '\nTodo OK')
process.exit(failed ? 1 : 0)
