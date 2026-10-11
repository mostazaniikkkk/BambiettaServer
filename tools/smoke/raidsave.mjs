// A raid survives a restart. Run "node raidsave.mjs start", wait for the autosave (60 s),
// restart the server, then "node raidsave.mjs check".
// Usage: node raidsave.mjs start|check [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'

const mode = process.argv[2]
const host = process.argv[3] ?? '127.0.0.1'
const port = Number(process.argv[4] ?? 25565)
let failures = 0
const check = (ok, msg) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${msg}`); if (!ok) failures++ }
const until = async (fn, ms = 4000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await fn()) return true; await sleep(200) } return false }
const a = mineflayer.createBot({ host, port, username: 'Defender', version: '1.20.4', auth: 'offline' })
let bar = null
a._client.on('boss_bar', p => { if (p.action === 0 && JSON.stringify(p.title).includes('raid')) bar = p })
await once(a, 'spawn'); await sleep(1500)
const pillagers = () => Object.values(a.entities).filter(e => e.name === 'pillager').length
if (mode === 'start') {
  let last = ''
  a.on('messagestr', m => { last = m })
  a.chat('/gamemode creative'); a.chat('/locate village'); await until(() => /village/.test(last), 3000)
  const [x, y, z] = last.match(/(-?\d+) (-?\d+) (-?\d+)/).slice(1).map(Number)
  a.chat(`/tp ${x} ${y + 30} ${z}`); await sleep(3000); a.chat(`/tp ${x} ${y + 2} ${z}`); await sleep(5000)
  a.chat('/effect bad_omen 600 1')
  check(await until(() => bar != null, 5000), 'empieza la incursión')
  check(await until(() => pillagers() > 0, 25000), `llega la primera oleada (${pillagers()})`)
} else {
  check(await until(() => bar != null, 5000), 'tras reiniciar, la incursión sigue (barra)')
  check(await until(() => pillagers() > 0, 5000), `y sus saqueadores siguen ahí (${pillagers()})`)
}
a.quit()
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
