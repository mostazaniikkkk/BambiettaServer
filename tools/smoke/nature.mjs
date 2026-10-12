// Nature: grass spreading and dying, ice melting next to torches, bamboo growing, bees
// (flowers and nectar, stings, honey harvest).
// Usage: node nature.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
import { Vec3 } from 'vec3'
const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failed = 0
const check = (ok, what) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${what}`); if (!ok) failed++ }
const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end; await sleep(100)) if (f()) return true; return false }
const b = mineflayer.createBot({ host, port, username: 'Naturalist', version: '1.20.4', auth: 'offline' })
b._client.on('registry_data', p => b.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
await once(b, 'spawn'); await sleep(1500)
const [X, Y, Z] = [50000, 150, 50000]
const at = (dx, dy, dz) => b.blockAt(new Vec3(X + dx, Y + dy, Z + dz))
b.chat('/gamemode creative'); b.chat('/time set 6000'); b.chat('/weather clear')
b.chat('/gamerule randomTickSpeed 200') // speeds everything up
b.chat(`/tp ${X} ${Y + 3} ${Z}`); await sleep(2500)
b.chat(`/fill ${X - 12} ${Y - 1} ${Z - 12} ${X + 12} ${Y - 1} ${Z + 12} dirt`)
b.chat(`/fill ${X - 12} ${Y} ${Z - 12} ${X + 12} ${Y + 8} ${Z + 12} air`); await sleep(800)

// 1. Grass spreads to the dirt next to it; under a block it dies.
b.chat(`/setblock ${X} ${Y - 1} ${Z} grass_block`)
b.chat(`/setblock ${X + 8} ${Y - 1} ${Z + 8} grass_block`)
b.chat(`/setblock ${X + 8} ${Y} ${Z + 8} stone`); await sleep(300)
const grassy = () => { let n = 0; for (let dx = -2; dx <= 2; dx++) for (let dz = -2; dz <= 2; dz++) if (at(dx, -1, dz)?.name === 'grass_block') n++; return n }
check(await until(() => grassy() >= 4, 20000), `el pasto se propaga a la tierra (${grassy()} bloques)`)
check(await until(() => at(8, -1, 8)?.name === 'dirt', 20000), 'bajo un bloque opaco el pasto vuelve a tierra')

// 2. Ice next to a torch melts.
b.chat(`/setblock ${X - 6} ${Y} ${Z} ice`)
b.chat(`/setblock ${X - 6} ${Y} ${Z + 1} torch`); await sleep(300)
check(await until(() => at(-6, 0, 0)?.name === 'water', 20000), `el hielo junto a una antorcha se derrite (${at(-6, 0, 0)?.name})`)

// 3. Bamboo: the shoot grows into a stalk.
b.chat(`/setblock ${X + 4} ${Y} ${Z - 6} bamboo_sapling`); await sleep(300)
const tall = () => { let h = 0; while (at(4, h, -6)?.name === 'bamboo') h++; return h }
check(await until(() => tall() >= 3, 30000), `el bambú crece (${tall()} de alto)`)
b.chat('/gamerule randomTickSpeed 3')

// 4. Bees: to the flowers, nectar; a hit makes them sting.
b.chat(`/fill ${X - 3} ${Y} ${Z + 6} ${X + 3} ${Y} ${Z + 8} poppy`); await sleep(300)
b.chat(`/tp ${X} ${Y} ${Z + 2}`); await sleep(300)
b.chat('/summon bee'); await sleep(500)
b.chat(`/tp ${X + 10} ${Y + 6} ${Z - 10}`)
const bee = Object.values(b.entities).filter(e => e.name === 'bee').sort((p, q) => p.position.distanceTo(new Vec3(X, Y, Z + 2)) - q.position.distanceTo(new Vec3(X, Y, Z + 2)))[0]
check(!!bee, 'aparece una abeja')
let nectar = false
b._client.on('entity_metadata', p => { if (p.entityId === bee?.id && p.metadata.some(m => m.key === 17 && (m.value & 8))) nectar = true })
check(await until(() => nectar, 25000), 'vuela a las flores y junta néctar')
// Honey: a full hive with shears gives honeycomb.
b.chat(`/setblock ${X - 4} ${Y} ${Z - 4} beehive[honey_level=5,facing=south]`); await sleep(300)
b.chat(`/tp ${X - 4} ${Y} ${Z - 2}`); await sleep(300)
b.chat('/give shears'); await until(() => b.inventory.items().some(i => i.name === 'shears'), 3000)
await b.equip(b.inventory.items().find(i => i.name === 'shears'), 'hand')
await b.activateBlock(at(-4, 0, -4)); await sleep(800)
check(at(-4, 0, -4)?.getProperties?.().honey_level == 0, `las tijeras vacían la colmena (${at(-4, 0, -4)?.getProperties?.().honey_level})`)
check(b.inventory.items().some(i => i.name === 'honeycomb') || Object.values(b.entities).some(e => e.name === 'item' && e.position.distanceTo(new Vec3(X - 4, Y, Z - 4)) < 3), 'sueltan panal')
// A hit: it gets angry and stings.
b.chat('/gamemode survival'); b.chat('/heal')
b.chat(`/tp ${bee.position.x.toFixed(1)} ${bee.position.y.toFixed(1)} ${(bee.position.z - 1.5).toFixed(1)}`); await sleep(400)
await b.lookAt(bee.position, true); b.attack(bee)
check(await until(() => b.health < 20, 15000), `la abeja enojada pica (vida ${b.health})`)
b.chat('/gamemode creative')
b.quit()
console.log(failed ? `\n${failed} fallaron` : '\nTodo OK')
process.exit(failed ? 1 : 0)
