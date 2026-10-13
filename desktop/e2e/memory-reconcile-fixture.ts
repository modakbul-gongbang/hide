// Invented Memory records in the fixture's private SQLite store, never operator data.
import { execFileSync } from "node:child_process";
import path from "node:path";
import type { Isolated } from "./fixture";

export function seedReconciliation(run: Isolated): void {
  execFileSync("python3", ["-c", String.raw`
import json, sqlite3, sys
db=sqlite3.connect(sys.argv[1], timeout=5)
project=db.execute('SELECT id FROM projects').fetchone()[0]
now=1791849600000
db.execute('UPDATE projects SET enabled=1,disclosure_version=3,next_number=11 WHERE id=?',(project,))
cards=[(1,'Run checks before merging','rule'),(2,'Use a pooled connection for writes','decision'),(3,'Keep session files for thirty days','rule'),(4,'Use the retired test command','lesson'),(5,'Prefer concise Korean replies','preference'),(6,'Bound the worker queue','rule'),(7,'Reject stale replies','rule'),(8,'Keep one owner per process','lesson'),(9,'Validate paths before writes','rule'),(10,'Keep output inside the checkout','rule')]
for number,title,kind in cards:
  item='fixture-m'+str(number)
  db.execute('INSERT INTO memory_items(id,project_id,number,lifecycle,current_revision,confidence,salience,created_at_ms,updated_at_ms,promotion_ok_revision) VALUES(?,?,?,\'active\',1,.9,.9,?,?,1)',(item,project,number,now,now))
  db.execute('INSERT INTO memory_revisions(id,item_id,revision,body,kind,lifecycle,created_at_ms,batch_id,title,verified_at_ms) VALUES(?,?,1,?,?,\'active\',?,\'fixture\',?,?)',(item+'-r1',item,title,kind,now,title,now))
  db.execute('INSERT INTO memory_fts(item_id,project_id,body,normalized_terms) VALUES(?,?,?,?)',(item,project,title,title.lower()))
  for session in ['fixture-source-one','fixture-source-two']:
    db.execute('INSERT INTO memory_sources(item_id,revision,provider,session_id,event_offset,content_hash,human,at_ms) VALUES(?,1,\'codex\',?,1,\'fixture\',1,?)',(item,session,now))
for number,authority in [(2,'card_newer'),(3,'ask'),(4,'outdated'),(8,'card_newer')]:
  comparison=dict(authority=authority,document='AGENTS.md',document_text='Use one unpooled connection per write.',document_at_unix_ms=None if number==3 else now-86400000 if number==2 else now+86400000,operator_text='Use a pooled connection for all writes.',operator_at_unix_ms=now,source_session='fixture-source-one')
  if number==4: comparison['document']='docs/architecture/repository-ownership-and-session-verification-guide.md'
  db.execute('INSERT INTO card_comparisons(item_id,revision,data) VALUES(?,1,?)',('fixture-m'+str(number),json.dumps(comparison)))
for number in [9,10]:
  db.execute('INSERT INTO card_promotions(item_id,state,revision,attempt,branch,pr_number,target_path,started_at_ms,updated_at_ms) VALUES(?,\'pr_open\',1,1,\'fixture-shared-pr\',72,\'AGENTS.md\',?,?)',('fixture-m'+str(number),now,now))
db.commit()
`, path.join(run.env.HIDE_STATE_DIR!, "project-memory.sqlite3")], { encoding: "utf8", timeout: 10_000 });
}
