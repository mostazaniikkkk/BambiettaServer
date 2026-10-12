// Server administration: game rules (keepInventory, doDaylightCycle), bans, IP bans,
// whitelist, world border.
// Usage: node admin.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'

// mineflayer race with fast servers: registry data can arrive before its plugins load.
const fixRace = b => { b._client.on('registry_data', p => b.registry?.loadDimensionCodec?.(p.codec || p)); return b }
const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failed = 0
const check = (ok, what) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${what}`); if (!ok) failed++ }
const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end; await sleep(100)) if (f()) return true; return false }
const bot = (username) => fixRace(mineflayer.createBot({ host, port, username, version: '1.20.4', auth: 'offline' }))
// Tries to join: 'spawn' or the kick reason.
const tryJoin = (name) => new Promise(resolve => {
  const c = bot(name)
  c.once('spawn', () => resolve({ ok: true, bot: c }))
  c.once('kicked', r => resolve({ ok: false, reason: typeof r === 'string' ? r : JSON.stringify(r) }))
  // The kick packet may still be in the client's parser when the socket closes.
  c.once('end', () => setTimeout(() => resolve({ ok: false, reason: 'end' }), 500))
  setTimeout(() => resolve({ ok: false, reason: 'timeout' }), 8000)
})
const a = bot('Admin')
await once(a, 'spawn'); await sleep(1500)
let last = ''
a.on('messagestr', m => { last = m })
const count = (name) => a.inventory.items().filter(i => i.name === name).reduce((n, i) => n + i.count, 0)

// 1. keepInventory.
a.chat('/gamemode creative'); a.chat('/tp 60000 150 60000'); await sleep(2000)
a.chat('/fill 59995 149 59995 60005 149 60005 stone'); a.chat('/clear'); await sleep(500)
a.chat('/give diamond 5'); await until(() => count('diamond') === 5, 3000)
a.chat('/gamerule keepInventory true'); a.chat('/gamemode survival'); await sleep(300)
a.chat('/kill'); await sleep(1500)
check(count('diamond') === 5, `con keepInventory se conserva el inventario al morir (${count('diamond')})`)
a.chat('/gamerule keepInventory false'); await sleep(300)
last = ''; a.chat('/gamerule keepInventory'); await until(() => last.includes('keepInventory'), 2000)
check(last.includes('false'), `/gamerule consulta el valor (${last})`)
// 2. doDaylightCycle.
// The time packet arrives once per second: wait for one sent after the change.
a.chat('/gamerule doDaylightCycle false'); await sleep(1500)
const t0 = a.time.timeOfDay
await sleep(3000)
check(Math.abs(a.time.timeOfDay - t0) < 5, `sin doDaylightCycle el tiempo no avanza (${t0} -> ${a.time.timeOfDay})`)
a.chat('/gamerule doDaylightCycle true'); a.chat('/gamemode creative'); await sleep(300)

// 3. Bans.
let j = await tryJoin('Intruder')
check(j.ok, 'Intruder entra')
const kicked = new Promise(r => j.bot.once('kicked', x => r(String(typeof x === 'string' ? x : JSON.stringify(x)))))
a.chat('/ban Intruder griefing')
check((await Promise.race([kicked, sleep(3000).then(() => '')])).length > 0, 'al banearlo es expulsado')
await sleep(1000)
j = await tryJoin('Intruder')
check(!j.ok && /banead|griefing/i.test(j.reason), `baneado no puede volver (${j.reason?.slice(0, 80)})`)
a.chat('/pardon Intruder'); await sleep(500)
j = await tryJoin('Intruder')
check(j.ok, 'tras /pardon puede volver')
j.bot?.quit(); await sleep(500)
last = ''; a.chat('/ban-ip 127.0.0.1'); await until(() => last.length > 0, 2000)
check(/túnel|local/.test(last), `no deja banear la IP local (${last.slice(0, 60)})`)

// 4. Whitelist.
a.chat('/whitelist on'); await sleep(500)
j = await tryJoin('Stranger')
check(!j.ok && /lista blanca/i.test(j.reason), `con la lista blanca activa un desconocido no entra (${j.reason?.slice(0, 60)})`)
a.chat('/whitelist add Stranger'); await sleep(500)
j = await tryJoin('Stranger')
check(j.ok, 'añadido a la lista blanca, entra')
j.bot?.quit()
a.chat('/whitelist remove Stranger'); a.chat('/whitelist off'); await sleep(500)

// 5. World border.
let border = null
a._client.on('initialize_world_border', p => { border = p })
a.chat('/worldborder center 60000 60000'); a.chat('/worldborder set 30'); await sleep(800)
check(border && Math.round(border.newDiameter) === 30, `el cliente recibe el borde (${border?.newDiameter})`)
a.chat('/gamemode survival'); a.chat('/heal'); await sleep(300)
a.chat('/tp 60040 150 60000'); await sleep(200)
a.chat('/fill 60036 149 59996 60044 149 60004 stone')
check(await until(() => a.health < 20, 4000), `fuera del borde hace daño (vida ${a.health})`)
a.chat('/gamemode creative'); a.chat('/worldborder set 59999968'); a.chat('/worldborder center 0 0'); a.chat('/heal')
await sleep(500)
a.quit()
console.log(failed ? `\n${failed} fallaron` : '\nTodo OK')
process.exit(failed ? 1 : 0)
