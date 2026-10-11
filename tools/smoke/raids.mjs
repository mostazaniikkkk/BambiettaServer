// Raids and illagers: Bad Omen in a village starts a raid (boss bar, waves from outside,
// raiders going for the villagers); beating every wave gives Hero of the Village. Also the
// evoker (fangs, vexes), the ravager and the Totem of Undying.
// Usage: node raids.mjs [host] [port]   (noise world with villages; difficulty easy = 3 waves)
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'

const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failures = 0
const check = (ok, msg) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${msg}`); if (!ok) failures++ }
const until = async (fn, ms = 4000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await fn()) return true; await sleep(200) } return false }
const step = async (name, fn) => { try { await fn() } catch (e) { check(false, `${name}: ${e.message}`) } }

const a = mineflayer.createBot({ host, port, username: 'Defender', version: '1.20.4', auth: 'offline' })
await once(a, 'spawn'); await a.waitForChunksToLoad().catch(() => {})
let last = ''
a.on('messagestr', m => { last = m })
const effects = new Set()
a._client.on('entity_effect', p => { if (p.entityId === a.entity.id) effects.add(a.registry.effects[p.effectId]?.name?.toLowerCase().replace(/_/g, '')) })
const bars = new Map()
a._client.on('boss_bar', p => {
  if (p.action === 0) bars.set(p.entityUUID, JSON.stringify(p.title))
  if (p.action === 3) bars.set(p.entityUUID, JSON.stringify(p.title))
  if (p.action === 1) bars.delete(p.entityUUID)
})
const raidBar = () => [...bars.values()].find(t => t.includes('event.minecraft.raid'))
const mobs = name => Object.values(a.entities).filter(e => e.name === name)
const RAIDERS = ['pillager', 'vindicator', 'witch', 'evoker', 'ravager']
a.chat('/gamemode creative'); a.chat('/gamerule doDaylightCycle false'); a.chat('/time set 1000')

await step('incursión', async () => {
  last = ''; a.chat('/locate village')
  await until(() => /village/.test(last), 3000)
  const m = last.match(/(-?\d+) (-?\d+) (-?\d+)/)
  check(m != null, `hay una aldea (${last})`)
  const [x, y, z] = [Number(m[1]), Number(m[2]), Number(m[3])]
  a.chat(`/tp ${x} ${y + 30} ${z}`); await sleep(3000)
  a.chat(`/tp ${x} ${y + 2} ${z}`); await sleep(3000)
  check(await until(() => mobs('villager').length >= 3, 10000), `la aldea tiene aldeanos (${mobs('villager').length})`)
  a.chat('/effect bad_omen 600 1')
  check(await until(() => raidBar() != null, 5000), 'con Mal presagio en la aldea empieza una incursión (barra)')
  let waves = 0
  for (let w = 1; w <= 4; w++) {
    // Every wave brings pillagers (a witch about could be a natural one).
    const came = await until(() => mobs('pillager').length > 0, 25000)
    if (!came) break
    waves++
    const kinds = RAIDERS.map(n => `${n}x${mobs(n).length}`).filter(s => !s.endsWith('x0')).join(' ')
    console.log(`     oleada ${w}: ${kinds}`)
    if (w === 1) {
      await sleep(500)
      check(mobs('pillager').length >= 4, 'la primera oleada trae saqueadores')
      // They head into the village.
      const p = mobs('pillager')[0]
      const d0 = Math.hypot(p.position.x - x, p.position.z - z)
      check(await until(() => Math.hypot(p.position.x - x, p.position.z - z) < d0 - 4, 15000), `los saqueadores van hacia la aldea (${d0.toFixed(0)} -> ${Math.hypot(p.position.x - x, p.position.z - z).toFixed(0)})`)
    }
    // The defenders win the wave.
    for (let i = 0; i < 5; i++) {
      for (const n of RAIDERS) a.chat(`/killmobs ${n} 160`)
      await sleep(600)
    }
  }
  check(waves >= 3, `llegan las oleadas de la dificultad (${waves})`)
  check(await until(() => (raidBar() ?? '').includes('victory'), 10000), 'al vencerlas: victoria')
  check(await until(() => effects.has('heroofthevillage'), 5000), 'y el defensor es Héroe de la aldea')
  // A hero gets a discount from the villagers.
  let offers = null
  a._client.once('trade_list', p => { offers = p.trades })
  const v = mobs('villager').sort((p, q) => p.position.distanceTo(a.entity.position) - q.position.distanceTo(a.entity.position))[0]
  if (v) {
    a.chat(`/tp ${v.position.x.toFixed(1)} ${v.position.y.toFixed(1)} ${(v.position.z + 1.5).toFixed(1)}`); await sleep(500)
    await a.lookAt(v.position.offset(0, 1.5, 0), true); await a.activateEntity(v)
  }
  check(await until(() => offers != null, 3000) && offers.every(o => o.specialPrice < 0), `los aldeanos le hacen descuento (${offers?.map(o => o.specialPrice).join(', ')})`)
  if (a.currentWindow) a.closeWindow(a.currentWindow)
})

await step('invocador', async () => {
  a.chat('/tp 9000.5 130 9000.5'); await sleep(1500)
  a.chat('/fill 8988 99 8988 9012 99 9012 stone'); a.chat('/fill 8988 100 8988 9012 106 9012 air'); await sleep(600)
  a.chat('/tp 9000.5 100 9000.5'); await sleep(800)
  a.chat('/summon evoker 9006.5 100 9000.5'); await sleep(300)
  a.chat('/gamemode survival'); a.chat('/heal'); await sleep(200)
  check(await until(() => mobs('evoker_fangs').length > 0, 10000), 'el invocador lanza colmillos')
  check(await until(() => mobs('vex').length > 0, 20000), `e invoca vexes (${mobs('vex').length})`)
  a.chat('/gamemode creative'); a.chat('/killmobs all 40'); a.chat('/heal')
})

await step('devastador', async () => {
  a.chat('/summon ravager 9004.5 100 9000.5'); await sleep(300)
  a.chat('/gamemode survival'); a.chat('/heal'); await sleep(200)
  const y0 = a.entity.position.y
  let high = y0
  const t = Date.now()
  while (Date.now() - t < 8000 && a.health >= 20) { high = Math.max(high, a.entity.position.y); await sleep(100) }
  check(a.health < 20, `el devastador embiste (vida ${a.health})`)
  a.chat('/gamemode creative'); a.chat('/killmobs all 40'); a.chat('/heal')
})

await step('tótem de la inmortalidad', async () => {
  a.chat('/give totem_of_undying'); await until(() => a.inventory.items().some(i => i.name === 'totem_of_undying'), 2000)
  await a.equip(a.inventory.items().find(i => i.name === 'totem_of_undying'), 'hand')
  let used = false
  a._client.on('entity_status', p => { if (p.entityId === a.entity.id && p.entityStatus === 35) used = true })
  a.chat('/gamemode survival'); a.chat('/heal'); effects.clear(); await sleep(300)
  a.chat('/effect instant_damage 1 5'); await sleep(800)
  check(used && a.health > 0, `el tótem salva de la muerte (vida ${a.health})`)
  check(effects.has('regeneration') && effects.has('absorption'), `y da regeneración y absorción (${[...effects].join(', ')})`)
  check(!a.inventory.items().some(i => i.name === 'totem_of_undying'), 'y se gasta')
  a.chat('/gamemode creative'); a.chat('/heal')
})

a.chat('/gamerule doDaylightCycle true')
await sleep(300)
a.quit()
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
