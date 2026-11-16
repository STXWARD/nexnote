use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::{collections::HashMap, fs, io::{Read, Write}, path::{Path, PathBuf}, sync::Mutex};
use tauri::Manager;
use uuid::Uuid;

struct Store { db: Mutex<Connection>, db_path: PathBuf, assets: PathBuf }

#[derive(Serialize)]
struct Book { id: String, title: String, description: String, color: String, created_at: String, updated_at: String, favorite: bool, chapter_count: i64, note_count: i64 }
#[derive(Serialize)]
struct Chapter { id: String, book_id: String, title: String, sort_order: i64, note_count: i64 }
#[derive(Serialize)]
struct Note { id: String, book_id: String, chapter_id: Option<String>, title: String, content: String, plain_text: String, updated_at: String, favorite: bool, tags: Vec<String> }
#[derive(Serialize)]
struct SearchResult { note_id: String, title: String, snippet: String, book_id: String, book_title: String, chapter_title: String }
#[derive(Serialize)]
struct Stats { books: i64, chapters: i64, notes: i64, attachments: i64, recordings: i64 }
#[derive(Serialize)]
struct Attachment { id:String, note_id:String, kind:String, name:String, path:String, mime_type:Option<String>, caption:Option<String>, created_at:String }

fn now() -> String { Utc::now().to_rfc3339() }
fn id() -> String { Uuid::new_v4().to_string() }
fn asset_path(store:&Store,path:&str)->PathBuf{let p=Path::new(path);if p.is_absolute(){p.to_path_buf()}else{store.assets.join(p.file_name().unwrap_or_default())}}
fn database(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = match std::env::var("NEXNOTE_DATA_DIR") {
        Ok(path) => PathBuf::from(path),
        Err(_) => app.path().app_data_dir().map_err(|e| e.to_string())?,
    };
    fs::create_dir_all(dir.join("assets")).map_err(|e| e.to_string())?;
    Ok(dir.join("nexnote.sqlite3"))
}
fn init(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;
      CREATE TABLE IF NOT EXISTS schema_migrations(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS books(id TEXT PRIMARY KEY,title TEXT NOT NULL,description TEXT NOT NULL DEFAULT '',color TEXT NOT NULL DEFAULT '#d9e6d5',created_at TEXT NOT NULL,updated_at TEXT NOT NULL,favorite INTEGER NOT NULL DEFAULT 0,deleted_at TEXT);
      CREATE TABLE IF NOT EXISTS chapters(id TEXT PRIMARY KEY,book_id TEXT NOT NULL REFERENCES books(id),parent_id TEXT REFERENCES chapters(id),title TEXT NOT NULL,sort_order INTEGER NOT NULL DEFAULT 0,created_at TEXT NOT NULL,updated_at TEXT NOT NULL,deleted_at TEXT);
      CREATE TABLE IF NOT EXISTS notes(id TEXT PRIMARY KEY,book_id TEXT NOT NULL REFERENCES books(id),chapter_id TEXT REFERENCES chapters(id),title TEXT NOT NULL,content TEXT NOT NULL DEFAULT '',sort_order INTEGER NOT NULL DEFAULT 0,created_at TEXT NOT NULL,updated_at TEXT NOT NULL,favorite INTEGER NOT NULL DEFAULT 0,deleted_at TEXT);
      CREATE TABLE IF NOT EXISTS attachments(id TEXT PRIMARY KEY,note_id TEXT NOT NULL REFERENCES notes(id),kind TEXT NOT NULL,name TEXT NOT NULL,path TEXT NOT NULL,mime_type TEXT,caption TEXT,created_at TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS tags(id TEXT PRIMARY KEY,name TEXT NOT NULL UNIQUE,color TEXT NOT NULL DEFAULT '#8d9b8e');
      CREATE TABLE IF NOT EXISTS note_tags(note_id TEXT NOT NULL REFERENCES notes(id),tag_id TEXT NOT NULL REFERENCES tags(id),PRIMARY KEY(note_id,tag_id));
      CREATE TABLE IF NOT EXISTS note_links(source_note_id TEXT NOT NULL REFERENCES notes(id),target_note_id TEXT NOT NULL REFERENCES notes(id),PRIMARY KEY(source_note_id,target_note_id));
      CREATE VIRTUAL TABLE IF NOT EXISTS note_search USING fts5(note_id UNINDEXED,title,content,tokenize='unicode61');
      CREATE TRIGGER IF NOT EXISTS notes_search_insert AFTER INSERT ON notes BEGIN INSERT INTO note_search(note_id,title,content) VALUES(new.id,new.title,new.content); END;
      CREATE TRIGGER IF NOT EXISTS notes_search_update AFTER UPDATE OF title,content ON notes BEGIN DELETE FROM note_search WHERE note_id=old.id; INSERT INTO note_search(note_id,title,content) VALUES(new.id,new.title,new.content); END;
      CREATE TRIGGER IF NOT EXISTS notes_search_delete AFTER DELETE ON notes BEGIN DELETE FROM note_search WHERE note_id=old.id; END;
      INSERT OR IGNORE INTO schema_migrations(version,applied_at) VALUES(1,datetime('now'));")?;
    let has_plain: i64 = conn.query_row("SELECT count(*) FROM pragma_table_info('notes') WHERE name='plain_text'", [], |r| r.get(0))?;
    if has_plain == 0 {
        conn.execute_batch("ALTER TABLE notes ADD COLUMN plain_text TEXT NOT NULL DEFAULT '';
          ALTER TABLE books ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;
          UPDATE books SET sort_order=(SELECT count(*) FROM books older WHERE older.created_at<books.created_at OR (older.created_at=books.created_at AND older.id<books.id));
          UPDATE notes SET sort_order=(SELECT count(*) FROM notes older WHERE older.book_id=notes.book_id AND older.chapter_id IS notes.chapter_id AND (older.created_at<notes.created_at OR (older.created_at=notes.created_at AND older.id<notes.id)));
          UPDATE notes SET plain_text=content;
          DROP TRIGGER IF EXISTS notes_search_insert; DROP TRIGGER IF EXISTS notes_search_update; DROP TRIGGER IF EXISTS notes_search_delete;
          DROP TABLE note_search; CREATE VIRTUAL TABLE note_search USING fts5(note_id UNINDEXED,title,content,tokenize='unicode61');
          INSERT INTO note_search(note_id,title,content) SELECT id,title,plain_text FROM notes WHERE deleted_at IS NULL;
          CREATE TRIGGER notes_search_insert AFTER INSERT ON notes BEGIN INSERT INTO note_search(note_id,title,content) VALUES(new.id,new.title,new.plain_text); END;
          CREATE TRIGGER notes_search_update AFTER UPDATE OF title,content,plain_text ON notes BEGIN DELETE FROM note_search WHERE note_id=old.id; INSERT INTO note_search(note_id,title,content) VALUES(new.id,new.title,new.plain_text); END;
          CREATE TRIGGER notes_search_delete AFTER DELETE ON notes BEGIN DELETE FROM note_search WHERE note_id=old.id; END;
          CREATE TABLE IF NOT EXISTS note_links(source_note_id TEXT NOT NULL REFERENCES notes(id),target_note_id TEXT NOT NULL REFERENCES notes(id),PRIMARY KEY(source_note_id,target_note_id));
          INSERT OR IGNORE INTO schema_migrations(version,applied_at) VALUES(2,datetime('now'));")?;
    }
    Ok(())
}
fn lock<'a>(s: &'a tauri::State<'a, Store>) -> Result<std::sync::MutexGuard<'a, Connection>, String> { s.db.lock().map_err(|e| e.to_string()) }

#[tauri::command]
fn list_books(s: tauri::State<Store>) -> Result<Vec<Book>, String> {
    let c=lock(&s)?; let mut q=c.prepare("SELECT b.id,b.title,b.description,b.color,b.created_at,b.updated_at,b.favorite,(SELECT count(*) FROM chapters ch WHERE ch.book_id=b.id AND ch.deleted_at IS NULL),(SELECT count(*) FROM notes n WHERE n.book_id=b.id AND n.deleted_at IS NULL) FROM books b WHERE b.deleted_at IS NULL ORDER BY b.sort_order,b.updated_at DESC") .map_err(|e|e.to_string())?;
    let result=q.query_map([],|r|Ok(Book{id:r.get(0)?,title:r.get(1)?,description:r.get(2)?,color:r.get(3)?,created_at:r.get(4)?,updated_at:r.get(5)?,favorite:r.get::<_,i64>(6)?!=0,chapter_count:r.get(7)?,note_count:r.get(8)?})).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string()); result
}
#[tauri::command]
fn stats(s:tauri::State<Store>)->Result<Stats,String>{let c=lock(&s)?;Ok(Stats{books:c.query_row("SELECT count(*) FROM books WHERE deleted_at IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?,chapters:c.query_row("SELECT count(*) FROM chapters WHERE deleted_at IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?,notes:c.query_row("SELECT count(*) FROM notes WHERE deleted_at IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?,attachments:c.query_row("SELECT count(*) FROM attachments a JOIN notes n ON n.id=a.note_id WHERE n.deleted_at IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?,recordings:c.query_row("SELECT count(*) FROM attachments a JOIN notes n ON n.id=a.note_id WHERE n.deleted_at IS NULL AND a.kind='audio'",[],|r|r.get(0)).map_err(|e|e.to_string())?})}
#[tauri::command]
fn create_book(s:tauri::State<Store>,title:String,description:String,color:String)->Result<String,String>{let c=lock(&s)?;let x=id();let t=now();let order:i64=c.query_row("SELECT COALESCE(MAX(sort_order),-1)+1 FROM books",[],|r|r.get(0)).map_err(|e|e.to_string())?;c.execute("INSERT INTO books(id,title,description,color,created_at,updated_at,sort_order)VALUES(?1,?2,?3,?4,?5,?5,?6)",params![x,title.trim(),description,color,t,order]).map_err(|e|e.to_string())?;Ok(x)}
#[tauri::command]
fn rename_book(s:tauri::State<Store>,book_id:String,title:String,description:String)->Result<(),String>{lock(&s)?.execute("UPDATE books SET title=?1,description=?2,updated_at=?3 WHERE id=?4",params![title.trim(),description,now(),book_id]).map(|_|()).map_err(|e|e.to_string())}
#[tauri::command]
fn delete_book(s:tauri::State<Store>,book_id:String)->Result<(),String>{let mut c=lock(&s)?;let tx=c.transaction().map_err(|e|e.to_string())?;let stamp=now();tx.execute("UPDATE books SET deleted_at=?1 WHERE id=?2 AND deleted_at IS NULL",params![stamp,book_id]).map_err(|e|e.to_string())?;tx.execute("UPDATE chapters SET deleted_at=?1 WHERE book_id=?2 AND deleted_at IS NULL",params![stamp,book_id]).map_err(|e|e.to_string())?;tx.execute("UPDATE notes SET deleted_at=?1 WHERE book_id=?2 AND deleted_at IS NULL",params![stamp,book_id]).map_err(|e|e.to_string())?;tx.commit().map_err(|e|e.to_string())}
#[tauri::command]
fn toggle_book_favorite(s:tauri::State<Store>,book_id:String)->Result<(),String>{lock(&s)?.execute("UPDATE books SET favorite=1-favorite WHERE id=?1 AND deleted_at IS NULL",[book_id]).map(|_|()).map_err(|e|e.to_string())}
#[tauri::command]
fn set_book_color(s:tauri::State<Store>,book_id:String,color:String)->Result<(),String>{lock(&s)?.execute("UPDATE books SET color=?1,updated_at=?2 WHERE id=?3 AND deleted_at IS NULL",params![color,now(),book_id]).map(|_|()).map_err(|e|e.to_string())}
#[tauri::command]
fn list_chapters(s:tauri::State<Store>,book_id:String)->Result<Vec<Chapter>,String>{let c=lock(&s)?;let mut q=c.prepare("SELECT c.id,c.book_id,c.title,c.sort_order,(SELECT count(*) FROM notes n WHERE n.chapter_id=c.id AND n.deleted_at IS NULL) FROM chapters c WHERE c.book_id=?1 AND c.deleted_at IS NULL ORDER BY c.sort_order,c.title").map_err(|e|e.to_string())?;let result=q.query_map([book_id],|r|Ok(Chapter{id:r.get(0)?,book_id:r.get(1)?,title:r.get(2)?,sort_order:r.get(3)?,note_count:r.get(4)?})).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string());result}
#[tauri::command]
fn create_chapter(s:tauri::State<Store>,book_id:String,title:String)->Result<String,String>{let c=lock(&s)?;let x=id();let t=now();let order:i64=c.query_row("SELECT COALESCE(MAX(sort_order),-1)+1 FROM chapters WHERE book_id=?1",[&book_id],|r|r.get(0)).map_err(|e|e.to_string())?;c.execute("INSERT INTO chapters(id,book_id,title,sort_order,created_at,updated_at)VALUES(?1,?2,?3,?4,?5,?5)",params![x,book_id,title.trim(),order,t]).map_err(|e|e.to_string())?;Ok(x)}
#[tauri::command]
fn rename_chapter(s:tauri::State<Store>,chapter_id:String,title:String)->Result<(),String>{lock(&s)?.execute("UPDATE chapters SET title=?1,updated_at=?2 WHERE id=?3",params![title.trim(),now(),chapter_id]).map(|_|()).map_err(|e|e.to_string())}
#[tauri::command]
fn duplicate_chapter(s:tauri::State<Store>,chapter_id:String)->Result<String,String>{
    let mut c=lock(&s)?;let tx=c.transaction().map_err(|e|e.to_string())?;
    let row:(String,String,Option<String>,i64)=tx.query_row("SELECT book_id,title,parent_id,sort_order FROM chapters WHERE id=?1 AND deleted_at IS NULL",[&chapter_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|e.to_string())?;
    let new=id();let stamp=now();tx.execute("INSERT INTO chapters(id,book_id,title,parent_id,sort_order,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?6)",params![new,row.0,row.1+" (copy)",row.2,row.3+1,stamp]).map_err(|e|e.to_string())?;
    let notes={let mut q=tx.prepare("SELECT id,title,content,plain_text,favorite,sort_order FROM notes WHERE chapter_id=?1 AND deleted_at IS NULL ORDER BY sort_order").map_err(|e|e.to_string())?;let rows=q.query_map([&chapter_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,i64>(4)?,r.get::<_,i64>(5)?))).map_err(|e|e.to_string())?;rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?};
    for(old,title,content,plain,favorite,sort)in notes{let note_id=id();tx.execute("INSERT INTO notes(id,book_id,chapter_id,title,content,plain_text,favorite,sort_order,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",params![note_id,row.0,new,title,content,plain,favorite,sort,stamp]).map_err(|e|e.to_string())?;tx.execute("INSERT OR IGNORE INTO note_tags(note_id,tag_id) SELECT ?1,tag_id FROM note_tags WHERE note_id=?2",params![note_id,old]).map_err(|e|e.to_string())?;tx.execute("INSERT INTO attachments(id,note_id,kind,name,path,mime_type,caption,created_at) SELECT lower(hex(randomblob(16))),?1,kind,name,path,mime_type,caption,created_at FROM attachments WHERE note_id=?2",params![note_id,old]).map_err(|e|e.to_string())?;}
    tx.commit().map_err(|e|e.to_string())?;Ok(new)
}
#[tauri::command]
fn delete_chapter(s:tauri::State<Store>,chapter_id:String)->Result<(),String>{let mut c=lock(&s)?;let tx=c.transaction().map_err(|e|e.to_string())?;let stamp=now();tx.execute("WITH RECURSIVE tree(id) AS (SELECT id FROM chapters WHERE id=?2 UNION ALL SELECT c.id FROM chapters c JOIN tree t ON c.parent_id=t.id) UPDATE chapters SET deleted_at=?1 WHERE id IN (SELECT id FROM tree) AND deleted_at IS NULL",params![stamp,chapter_id]).map_err(|e|e.to_string())?;tx.execute("UPDATE notes SET deleted_at=?1 WHERE chapter_id IN (WITH RECURSIVE tree(id) AS (SELECT id FROM chapters WHERE id=?2 UNION ALL SELECT c.id FROM chapters c JOIN tree t ON c.parent_id=t.id) SELECT id FROM tree) AND deleted_at IS NULL",params![stamp,chapter_id]).map_err(|e|e.to_string())?;tx.commit().map_err(|e|e.to_string())}
#[tauri::command]
fn move_chapter(s:tauri::State<Store>,chapter_id:String,book_id:String)->Result<(),String>{let mut c=lock(&s)?;let tx=c.transaction().map_err(|e|e.to_string())?;let active:i64=tx.query_row("SELECT count(*) FROM books WHERE id=?1 AND deleted_at IS NULL",[&book_id],|r|r.get(0)).map_err(|e|e.to_string())?;if active==0{return Err("Choose an active destination book.".into())}let root:(String,i64)=tx.query_row("SELECT book_id,sort_order FROM chapters WHERE id=?1 AND deleted_at IS NULL",[&chapter_id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;let order:i64=tx.query_row("SELECT COALESCE(MAX(sort_order),-1)+1 FROM chapters WHERE book_id=?1",[&book_id],|r|r.get(0)).map_err(|e|e.to_string())?;tx.execute("WITH RECURSIVE tree(id) AS (SELECT id FROM chapters WHERE id=?1 UNION ALL SELECT c.id FROM chapters c JOIN tree t ON c.parent_id=t.id) UPDATE chapters SET book_id=?2 WHERE id IN (SELECT id FROM tree)",params![chapter_id,book_id]).map_err(|e|e.to_string())?;tx.execute("UPDATE chapters SET parent_id=NULL,sort_order=?1 WHERE id=?2",params![order,chapter_id]).map_err(|e|e.to_string())?;tx.execute("UPDATE notes SET book_id=?1 WHERE chapter_id IN (WITH RECURSIVE tree(id) AS (SELECT id FROM chapters WHERE id=?2 UNION ALL SELECT c.id FROM chapters c JOIN tree t ON c.parent_id=t.id) SELECT id FROM tree)",params![book_id,chapter_id]).map_err(|e|e.to_string())?;let _=root;tx.commit().map_err(|e|e.to_string())}
#[tauri::command]
fn reorder_chapter(s:tauri::State<Store>,chapter_id:String,direction:i64)->Result<(),String>{let c=lock(&s)?;let row:(String,Option<String>,i64)=c.query_row("SELECT book_id,parent_id,sort_order FROM chapters WHERE id=?1",[&chapter_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;let other:Option<String>=c.query_row("SELECT id FROM chapters WHERE book_id=?1 AND parent_id IS ?2 AND sort_order=?3 AND deleted_at IS NULL LIMIT 1",params![row.0,row.1,row.2+direction],|r|r.get(0)).optional().map_err(|e|e.to_string())?;if let Some(other_id)=other{c.execute("UPDATE chapters SET sort_order=?1 WHERE id=?2",params![row.2+direction,chapter_id]).map_err(|e|e.to_string())?;c.execute("UPDATE chapters SET sort_order=?1 WHERE id=?2",params![row.2,other_id]).map_err(|e|e.to_string())?;}Ok(())}
#[tauri::command]
fn list_notes(s:tauri::State<Store>,book_id:String)->Result<Vec<Note>,String>{let c=lock(&s)?;let mut q=c.prepare("SELECT n.id,n.book_id,n.chapter_id,n.title,n.content,n.plain_text,n.updated_at,n.favorite,COALESCE((SELECT group_concat(t.name,char(31)) FROM tags t JOIN note_tags nt ON nt.tag_id=t.id WHERE nt.note_id=n.id),'') FROM notes n WHERE n.book_id=?1 AND n.deleted_at IS NULL ORDER BY n.sort_order,n.updated_at DESC").map_err(|e|e.to_string())?;let result=q.query_map([book_id],|r|{let tag_string:String=r.get(8)?;let tags=if tag_string.is_empty(){Vec::new()}else{tag_string.split('\u{1f}').map(str::to_owned).collect()};Ok(Note{id:r.get(0)?,book_id:r.get(1)?,chapter_id:r.get(2)?,title:r.get(3)?,content:r.get(4)?,plain_text:r.get(5)?,updated_at:r.get(6)?,favorite:r.get::<_,i64>(7)?!=0,tags})}).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string());result}
#[tauri::command]
fn dashboard_notes(s:tauri::State<Store>)->Result<Vec<Note>,String>{let c=lock(&s)?;let mut q=c.prepare("SELECT n.id,n.book_id,n.chapter_id,n.title,n.content,n.plain_text,n.updated_at,n.favorite,COALESCE((SELECT group_concat(t.name,char(31)) FROM tags t JOIN note_tags nt ON nt.tag_id=t.id WHERE nt.note_id=n.id),'') FROM notes n JOIN books b ON b.id=n.book_id WHERE n.deleted_at IS NULL AND b.deleted_at IS NULL ORDER BY n.updated_at DESC LIMIT 50").map_err(|e|e.to_string())?;let result=q.query_map([],|r|{let tag_string:String=r.get(8)?;let tags=if tag_string.is_empty(){Vec::new()}else{tag_string.split('\u{1f}').map(str::to_owned).collect()};Ok(Note{id:r.get(0)?,book_id:r.get(1)?,chapter_id:r.get(2)?,title:r.get(3)?,content:r.get(4)?,plain_text:r.get(5)?,updated_at:r.get(6)?,favorite:r.get::<_,i64>(7)?!=0,tags})}).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string());result}
#[tauri::command]
fn create_note(s:tauri::State<Store>,book_id:String,chapter_id:Option<String>)->Result<String,String>{let c=lock(&s)?;let x=id();let t=now();let order:i64=c.query_row("SELECT COALESCE(MAX(sort_order),-1)+1 FROM notes WHERE book_id=?1 AND chapter_id IS ?2",params![book_id,chapter_id],|r|r.get(0)).map_err(|e|e.to_string())?;c.execute("INSERT INTO notes(id,book_id,chapter_id,title,content,plain_text,sort_order,created_at,updated_at)VALUES(?1,?2,?3,'Untitled note','','',?4,?5,?5)",params![x,book_id,chapter_id,order,t]).map_err(|e|e.to_string())?;Ok(x)}
#[tauri::command]
fn create_note_template(s:tauri::State<Store>,book_id:String,chapter_id:Option<String>,template:String)->Result<String,String>{let (title,html,plain)=match template.as_str(){"programming"=>("Programming note", "<h2>Concept</h2><p></p><h2>Example</h2><pre><code class=\"language-python\"></code></pre><h2>Output</h2><p></p>","Concept\nExample\nOutput"),"sql"=>("SQL query note","<h2>Purpose</h2><p></p><h2>Query</h2><pre><code class=\"language-sql\"></code></pre><h2>Expected result</h2><p></p>","Purpose\nQuery\nExpected result"),"interview"=>("Interview preparation","<h2>Question</h2><p></p><h2>Short answer</h2><p></p><h2>Explanation</h2><p></p><h2>Example</h2><p></p>","Question\nShort answer\nExplanation\nExample"),"daily"=>("Daily journal","<h2>Today</h2><p></p><h2>Highlights</h2><ul><li><p></p></li></ul><h2>Tomorrow</h2><p></p>","Today\nHighlights\nTomorrow"),"study"=>("Study notes","<h2>Topic</h2><p></p><h2>Key ideas</h2><ul><li><p></p></li></ul><h2>Questions</h2><p></p>","Topic\nKey ideas\nQuestions"),"meeting"=>("Meeting notes","<h2>Purpose</h2><p></p><h2>Discussion</h2><ul><li><p></p></li></ul><h2>Decisions</h2><p></p><h2>Next actions</h2><ul><li><p></p></li></ul>","Purpose\nDiscussion\nDecisions\nNext actions"),"journal"=>("Daily journal","<h2>Today</h2><p></p><h2>What mattered?</h2><p></p><h2>Tomorrow</h2><p></p>","Today\nWhat mattered?\nTomorrow"),"research"=>("Research notes","<h2>Question</h2><p></p><h2>Sources</h2><ul><li><p></p></li></ul><h2>Findings</h2><p></p><h2>Open questions</h2><p></p>","Question\nSources\nFindings\nOpen questions"),"project"=>("Project notes","<h2>Goal</h2><p></p><h2>Plan</h2><ul><li><p></p></li></ul><h2>Progress</h2><p></p><h2>Next steps</h2><p></p>","Goal\nPlan\nProgress\nNext steps"),"reading"=>("Reading notes","<h2>Book or article</h2><p></p><h2>Main ideas</h2><ul><li><p></p></li></ul><h2>Quotes and reflections</h2><p></p>","Book or article\nMain ideas\nQuotes and reflections"),"personal"=>("Personal notes","<h2>Thoughts</h2><p></p><h2>Things to remember</h2><ul><li><p></p></li></ul>","Thoughts\nThings to remember"),_=>return Err("Unknown note template.".into())};let c=lock(&s)?;let x=id();let t=now();let order:i64=c.query_row("SELECT COALESCE(MAX(sort_order),-1)+1 FROM notes WHERE book_id=?1 AND chapter_id IS ?2",params![book_id,chapter_id],|r|r.get(0)).map_err(|e|e.to_string())?;c.execute("INSERT INTO notes(id,book_id,chapter_id,title,content,plain_text,sort_order,created_at,updated_at)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?8)",params![x,book_id,chapter_id,title,html,plain,order,t]).map_err(|e|e.to_string())?;Ok(x)}
#[tauri::command]
fn duplicate_book(s:tauri::State<Store>,book_id:String)->Result<String,String>{
    let c=lock(&s)?;
    let tx=c.unchecked_transaction().map_err(|e|e.to_string())?;
    let info:(String,String,String)=tx.query_row("SELECT title,description,color FROM books WHERE id=?1 AND deleted_at IS NULL",[&book_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
    let new_book=id();
    let timestamp=now();
    let order:i64=tx.query_row("SELECT COALESCE(MAX(sort_order),-1)+1 FROM books",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    tx.execute("INSERT INTO books(id,title,description,color,created_at,updated_at,sort_order) VALUES(?1,?2,?3,?4,?5,?5,?6)",params![new_book,format!("{} (copy)",info.0),info.1,info.2,timestamp,order]).map_err(|e|e.to_string())?;
    let chapters={
        let mut q=tx.prepare("SELECT id,parent_id,title,sort_order,created_at,updated_at FROM chapters WHERE book_id=?1 AND deleted_at IS NULL ORDER BY sort_order").map_err(|e|e.to_string())?;
        let rows=q.query_map([&book_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?))).map_err(|e|e.to_string())?;
        let values=rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        values
    };
    let chapter_ids:HashMap<String,String>=chapters.iter().map(|row|(row.0.clone(),id())).collect();
    for(old,parent,title,sort,created,updated) in chapters{
        let new=chapter_ids.get(&old).expect("chapter id mapping");
        let mapped=parent.and_then(|p|chapter_ids.get(&p).cloned());
        tx.execute("INSERT INTO chapters(id,book_id,parent_id,title,sort_order,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![new,new_book,mapped,title,sort,created,updated]).map_err(|e|e.to_string())?;
    }
    let notes={
        let mut q=tx.prepare("SELECT id,chapter_id,title,content,plain_text,sort_order,created_at,updated_at,favorite FROM notes WHERE book_id=?1 AND deleted_at IS NULL ORDER BY sort_order").map_err(|e|e.to_string())?;
        let rows=q.query_map([&book_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,i64>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?,r.get::<_,i64>(8)?))).map_err(|e|e.to_string())?;
        let values=rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        values
    };
    let mut note_ids=HashMap::new();
    for(old,chapter,title,content,plain,sort,created,updated,favorite) in notes{
        let new=id();
        note_ids.insert(old.clone(),new.clone());
        let mapped=chapter.and_then(|p|chapter_ids.get(&p).cloned());
        tx.execute("INSERT INTO notes(id,book_id,chapter_id,title,content,plain_text,sort_order,created_at,updated_at,favorite) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![new,new_book,mapped,title,content,plain,sort,created,updated,favorite]).map_err(|e|e.to_string())?;
        tx.execute("INSERT INTO attachments(id,note_id,kind,name,path,mime_type,caption,created_at) SELECT lower(hex(randomblob(16))),?1,kind,name,path,mime_type,caption,created_at FROM attachments WHERE note_id=?2",params![new,old]).map_err(|e|e.to_string())?;
    }
    let tags={
        let mut q=tx.prepare("SELECT nt.note_id,nt.tag_id FROM note_tags nt JOIN notes n ON n.id=nt.note_id WHERE n.book_id=?1 AND n.deleted_at IS NULL").map_err(|e|e.to_string())?;
        let rows=q.query_map([&book_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(|e|e.to_string())?;
        let values=rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        values
    };
    for(old_note,tag) in tags{if let Some(new_note)=note_ids.get(&old_note){tx.execute("INSERT OR IGNORE INTO note_tags(note_id,tag_id) VALUES(?1,?2)",params![new_note,tag]).map_err(|e|e.to_string())?;}}
    let links={
        let mut q=tx.prepare("SELECT source_note_id,target_note_id FROM note_links WHERE source_note_id IN (SELECT id FROM notes WHERE book_id=?1) AND target_note_id IN (SELECT id FROM notes WHERE book_id=?1)").map_err(|e|e.to_string())?;
        let rows=q.query_map([&book_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(|e|e.to_string())?;
        let values=rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        values
    };
    for(source,target) in links{if let(Some(a),Some(b))=(note_ids.get(&source),note_ids.get(&target)){tx.execute("INSERT OR IGNORE INTO note_links(source_note_id,target_note_id) VALUES(?1,?2)",params![a,b]).map_err(|e|e.to_string())?;}}
    tx.commit().map_err(|e|e.to_string())?;
    Ok(new_book)
}
#[tauri::command]
fn duplicate_note(s:tauri::State<Store>,note_id:String)->Result<String,String>{let mut c=lock(&s)?;let tx=c.transaction().map_err(|e|e.to_string())?;let x=id();let t=now();let changed=tx.execute("INSERT INTO notes(id,book_id,chapter_id,title,content,plain_text,sort_order,created_at,updated_at,favorite) SELECT ?1,book_id,chapter_id,title||' (copy)',content,plain_text,sort_order+1,?2,?2,0 FROM notes WHERE id=?3 AND deleted_at IS NULL",params![x,t,note_id]).map_err(|e|e.to_string())?;if changed==0{return Err("The note no longer exists.".into())}tx.execute("INSERT OR IGNORE INTO note_tags(note_id,tag_id) SELECT ?1,tag_id FROM note_tags WHERE note_id=?2",params![x,note_id]).map_err(|e|e.to_string())?;tx.execute("INSERT INTO attachments(id,note_id,kind,name,path,mime_type,caption,created_at) SELECT lower(hex(randomblob(16))),?1,kind,name,path,mime_type,caption,created_at FROM attachments WHERE note_id=?2",params![x,note_id]).map_err(|e|e.to_string())?;tx.commit().map_err(|e|e.to_string())?;Ok(x)}
#[tauri::command]
fn move_note(s:tauri::State<Store>,note_id:String,book_id:String,chapter_id:Option<String>)->Result<(),String>{let c=lock(&s)?;if let Some(ref chapter)=chapter_id{let owner:Option<String>=c.query_row("SELECT book_id FROM chapters WHERE id=?1 AND deleted_at IS NULL",[chapter],|r|r.get(0)).optional().map_err(|e|e.to_string())?;if owner.as_deref()!=Some(book_id.as_str()){return Err("The selected chapter does not belong to that book.".into())}}let order:i64=c.query_row("SELECT COALESCE(MAX(sort_order),-1)+1 FROM notes WHERE book_id=?1 AND chapter_id IS ?2",params![book_id,chapter_id],|r|r.get(0)).map_err(|e|e.to_string())?;c.execute("UPDATE notes SET book_id=?1,chapter_id=?2,sort_order=?3,updated_at=?4 WHERE id=?5",params![book_id,chapter_id,order,now(),note_id]).map(|_|()).map_err(|e|e.to_string())}
#[tauri::command]
fn reorder_book(s:tauri::State<Store>,book_id:String,direction:i64)->Result<(),String>{let c=lock(&s)?;let pos:i64=c.query_row("SELECT sort_order FROM books WHERE id=?1",[&book_id],|r|r.get(0)).map_err(|e|e.to_string())?;let target=pos+direction;let other:Option<String>=c.query_row("SELECT id FROM books WHERE sort_order=?1 AND deleted_at IS NULL LIMIT 1",[target],|r|r.get(0)).optional().map_err(|e|e.to_string())?;if let Some(other_id)=other{c.execute("UPDATE books SET sort_order=?1 WHERE id=?2",params![target,book_id]).map_err(|e|e.to_string())?;c.execute("UPDATE books SET sort_order=?1 WHERE id=?2",params![pos,other_id]).map_err(|e|e.to_string())?;}Ok(())}
#[tauri::command]
fn reorder_note(s:tauri::State<Store>,note_id:String,direction:i64)->Result<(),String>{let c=lock(&s)?;let row:(String,Option<String>,i64)=c.query_row("SELECT book_id,chapter_id,sort_order FROM notes WHERE id=?1",[&note_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;let other:Option<String>=c.query_row("SELECT id FROM notes WHERE book_id=?1 AND chapter_id IS ?2 AND sort_order=?3 AND deleted_at IS NULL LIMIT 1",params![row.0,row.1,row.2+direction],|r|r.get(0)).optional().map_err(|e|e.to_string())?;if let Some(other_id)=other{c.execute("UPDATE notes SET sort_order=?1 WHERE id=?2",params![row.2+direction,note_id]).map_err(|e|e.to_string())?;c.execute("UPDATE notes SET sort_order=?1 WHERE id=?2",params![row.2,other_id]).map_err(|e|e.to_string())?;}Ok(())}
#[tauri::command]
fn set_note_tags(s:tauri::State<Store>,note_id:String,tags:Vec<String>)->Result<(),String>{let c=lock(&s)?;let tx=c.unchecked_transaction().map_err(|e|e.to_string())?;tx.execute("DELETE FROM note_tags WHERE note_id=?1",[&note_id]).map_err(|e|e.to_string())?;for name in tags.iter().map(|x|x.trim()).filter(|x|!x.is_empty()).take(20){let tag_id=id();tx.execute("INSERT INTO tags(id,name) VALUES(?1,?2) ON CONFLICT(name) DO NOTHING",params![tag_id,name]).map_err(|e|e.to_string())?;let tid:String=tx.query_row("SELECT id FROM tags WHERE name=?1",[name],|r|r.get(0)).map_err(|e|e.to_string())?;tx.execute("INSERT OR IGNORE INTO note_tags(note_id,tag_id) VALUES(?1,?2)",params![note_id,tid]).map_err(|e|e.to_string())?;}tx.commit().map_err(|e|e.to_string())}
#[tauri::command]
fn link_note(s:tauri::State<Store>,source_note_id:String,target_note_id:String)->Result<(),String>{lock(&s)?.execute("INSERT OR IGNORE INTO note_links(source_note_id,target_note_id) VALUES(?1,?2)",params![source_note_id,target_note_id]).map(|_|()).map_err(|e|e.to_string())}
#[tauri::command]
fn unlink_note(s:tauri::State<Store>,source_note_id:String,target_note_id:String)->Result<(),String>{lock(&s)?.execute("DELETE FROM note_links WHERE source_note_id=?1 AND target_note_id=?2",params![source_note_id,target_note_id]).map(|_|()).map_err(|e|e.to_string())}
#[tauri::command]
fn save_note(s:tauri::State<Store>,note_id:String,title:String,content:String,plain_text:String)->Result<(),String>{let c=lock(&s)?;let t=now();let book_id:String=c.query_row("SELECT book_id FROM notes WHERE id=?1",[&note_id],|r|r.get(0)).map_err(|e|e.to_string())?;c.execute("UPDATE notes SET title=?1,content=?2,plain_text=?3,updated_at=?4 WHERE id=?5",params![title,content,plain_text,t,note_id]).map_err(|e|e.to_string())?;c.execute("UPDATE books SET updated_at=?1 WHERE id=?2",params![t,book_id]).map_err(|e|e.to_string())?;Ok(())}
#[tauri::command]
fn delete_note(s:tauri::State<Store>,note_id:String)->Result<(),String>{lock(&s)?.execute("UPDATE notes SET deleted_at=?1 WHERE id=?2",params![now(),note_id]).map(|_|()).map_err(|e|e.to_string())}
#[tauri::command]
fn toggle_favorite(s:tauri::State<Store>,note_id:String)->Result<(),String>{lock(&s)?.execute("UPDATE notes SET favorite=1-favorite WHERE id=?1",[note_id]).map(|_|()).map_err(|e|e.to_string())}
#[tauri::command]
fn search_notes(s:tauri::State<Store>,query:String,book_id:Option<String>,from_date:Option<String>,to_date:Option<String>)->Result<Vec<SearchResult>,String>{let c=lock(&s)?;let term=query.split_whitespace().map(|w|format!("\"{}\"*",w.replace('"',""))).collect::<Vec<_>>().join(" AND ");if term.is_empty(){return Ok(vec![])}let like=format!("%{}%",query);let mut q=c.prepare("SELECT n.id,n.title,snippet(note_search,2,'','', ' … ',18),b.id,b.title,COALESCE(ch.title,'Unsorted') FROM note_search f JOIN notes n ON n.id=f.note_id JOIN books b ON b.id=n.book_id LEFT JOIN chapters ch ON ch.id=n.chapter_id WHERE ((note_search MATCH ?1) OR b.title LIKE ?2 OR ch.title LIKE ?2 OR EXISTS(SELECT 1 FROM attachments a WHERE a.note_id=n.id AND (a.name LIKE ?2 OR a.caption LIKE ?2)) OR EXISTS(SELECT 1 FROM note_tags nt JOIN tags t ON t.id=nt.tag_id WHERE nt.note_id=n.id AND t.name LIKE ?2)) AND (?3 IS NULL OR b.id=?3) AND (?4 IS NULL OR date(n.updated_at)>=date(?4)) AND (?5 IS NULL OR date(n.updated_at)<=date(?5)) AND n.deleted_at IS NULL AND b.deleted_at IS NULL ORDER BY n.updated_at DESC LIMIT 100").map_err(|e|e.to_string())?;let result=q.query_map(params![term,like,book_id,from_date,to_date],|r|Ok(SearchResult{note_id:r.get(0)?,title:r.get(1)?,snippet:r.get(2)?,book_id:r.get(3)?,book_title:r.get(4)?,chapter_title:r.get(5)?})).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string());result}
#[tauri::command]
fn trash_items(s:tauri::State<Store>)->Result<Vec<serde_json::Value>,String>{let c=lock(&s)?;let mut q=c.prepare("SELECT id,title,'book',deleted_at,'' AS location FROM books WHERE deleted_at IS NOT NULL UNION ALL SELECT ch.id,ch.title,'chapter',ch.deleted_at,b.title FROM chapters ch JOIN books b ON b.id=ch.book_id LEFT JOIN chapters parent ON parent.id=ch.parent_id WHERE ch.deleted_at IS NOT NULL AND b.deleted_at IS NULL AND (parent.id IS NULL OR parent.deleted_at IS NULL) UNION ALL SELECT n.id,n.title,'note',n.deleted_at,CASE WHEN ch.title IS NULL THEN b.title ELSE b.title||' / '||ch.title END FROM notes n JOIN books b ON b.id=n.book_id LEFT JOIN chapters ch ON ch.id=n.chapter_id WHERE n.deleted_at IS NOT NULL AND b.deleted_at IS NULL AND (ch.id IS NULL OR ch.deleted_at IS NULL) ORDER BY 4 DESC").map_err(|e|e.to_string())?;let rows=q.query_map([],|r|Ok(serde_json::json!({"id":r.get::<_,String>(0)? ,"title":r.get::<_,String>(1)?,"kind":r.get::<_,String>(2)?,"deleted_at":r.get::<_,String>(3)?,"location":r.get::<_,String>(4)?}))).map_err(|e|e.to_string())?;rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())}
#[tauri::command]
fn restore_item(s:tauri::State<Store>,item_id:String,kind:String)->Result<(),String>{let mut c=lock(&s)?;let tx=c.transaction().map_err(|e|e.to_string())?;match kind.as_str(){"book"=>{let stamp:Option<String>=tx.query_row("SELECT deleted_at FROM books WHERE id=?1",[&item_id],|r|r.get(0)).map_err(|e|e.to_string())?;if let Some(stamp)=stamp{tx.execute("UPDATE books SET deleted_at=NULL WHERE id=?1",[&item_id]).map_err(|e|e.to_string())?;tx.execute("UPDATE chapters SET deleted_at=NULL WHERE book_id=?1 AND deleted_at=?2",params![item_id,stamp]).map_err(|e|e.to_string())?;tx.execute("UPDATE notes SET deleted_at=NULL WHERE book_id=?1 AND deleted_at=?2",params![item_id,stamp]).map_err(|e|e.to_string())?;}},"chapter"=>{let row:Option<(String,String)>=tx.query_row("SELECT book_id,deleted_at FROM chapters WHERE id=?1 AND deleted_at IS NOT NULL",[&item_id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;if let Some((book,stamp))=row{let active:i64=tx.query_row("SELECT count(*) FROM books WHERE id=?1 AND deleted_at IS NULL",[book],|r|r.get(0)).map_err(|e|e.to_string())?;if active==0{return Err("Restore the parent book first.".into())}tx.execute("WITH RECURSIVE tree(id) AS (SELECT id FROM chapters WHERE id=?1 UNION ALL SELECT c.id FROM chapters c JOIN tree t ON c.parent_id=t.id) UPDATE chapters SET deleted_at=NULL WHERE id IN (SELECT id FROM tree) AND deleted_at=?2",params![item_id,stamp]).map_err(|e|e.to_string())?;tx.execute("UPDATE notes SET deleted_at=NULL WHERE chapter_id IN (WITH RECURSIVE tree(id) AS (SELECT id FROM chapters WHERE id=?1 UNION ALL SELECT c.id FROM chapters c JOIN tree t ON c.parent_id=t.id) SELECT id FROM tree) AND deleted_at=?2",params![item_id,stamp]).map_err(|e|e.to_string())?;}},"note"=>{let row:Option<(String,Option<String>)>=tx.query_row("SELECT book_id,chapter_id FROM notes WHERE id=?1",[&item_id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;if let Some((book,chapter))=row{let active:i64=tx.query_row("SELECT count(*) FROM books WHERE id=?1 AND deleted_at IS NULL",[book],|r|r.get(0)).map_err(|e|e.to_string())?;if active==0{return Err("Restore the parent book first.".into())}if let Some(chapter)=chapter{let active:i64=tx.query_row("SELECT count(*) FROM chapters WHERE id=?1 AND deleted_at IS NULL",[chapter],|r|r.get(0)).map_err(|e|e.to_string())?;if active==0{return Err("Restore the parent chapter first.".into())}}tx.execute("UPDATE notes SET deleted_at=NULL WHERE id=?1",[&item_id]).map_err(|e|e.to_string())?;}},_=>return Err("Unknown trash item type.".into())}tx.commit().map_err(|e|e.to_string())}

fn collect_note_ids(c:&Connection,sql:&str,param:&str)->Result<Vec<String>,String>{let mut q=c.prepare(sql).map_err(|e|e.to_string())?;let rows=q.query_map([param],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?;rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())}
fn hard_delete_notes(tx:&rusqlite::Transaction<'_>,note_ids:&[String],files:&mut Vec<String>)->Result<(),String>{for note_id in note_ids{let mut q=tx.prepare("SELECT path FROM attachments WHERE note_id=?1").map_err(|e|e.to_string())?;let rows=q.query_map([note_id],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?;files.extend(rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?);tx.execute("DELETE FROM note_links WHERE source_note_id=?1 OR target_note_id=?1",[note_id]).map_err(|e|e.to_string())?;tx.execute("DELETE FROM note_tags WHERE note_id=?1",[note_id]).map_err(|e|e.to_string())?;tx.execute("DELETE FROM attachments WHERE note_id=?1",[note_id]).map_err(|e|e.to_string())?;tx.execute("DELETE FROM notes WHERE id=?1",[note_id]).map_err(|e|e.to_string())?;}Ok(())}
fn hard_delete_item(tx:&rusqlite::Transaction<'_>,item_id:&str,kind:&str,files:&mut Vec<String>)->Result<(),String>{match kind{"note"=>hard_delete_notes(tx,&[item_id.to_owned()],files),"chapter"=>{let chapter_ids=collect_note_ids(tx,"WITH RECURSIVE tree(id) AS (SELECT id FROM chapters WHERE id=?1 UNION ALL SELECT c.id FROM chapters c JOIN tree t ON c.parent_id=t.id) SELECT id FROM tree",item_id)?;let notes=collect_note_ids(tx,"SELECT id FROM notes WHERE chapter_id IN (WITH RECURSIVE tree(id) AS (SELECT id FROM chapters WHERE id=?1 UNION ALL SELECT c.id FROM chapters c JOIN tree t ON c.parent_id=t.id) SELECT id FROM tree)",item_id)?;hard_delete_notes(tx,&notes,files)?;for chapter_id in &chapter_ids{tx.execute("UPDATE chapters SET parent_id=NULL WHERE id=?1",[chapter_id]).map_err(|e|e.to_string())?;}for chapter_id in &chapter_ids{tx.execute("DELETE FROM chapters WHERE id=?1",[chapter_id]).map_err(|e|e.to_string())?;}Ok(())},"book"=>{let notes=collect_note_ids(tx,"SELECT id FROM notes WHERE book_id=?1",item_id)?;hard_delete_notes(tx,&notes,files)?;tx.execute("UPDATE chapters SET parent_id=NULL WHERE book_id=?1",[item_id]).map_err(|e|e.to_string())?;tx.execute("DELETE FROM chapters WHERE book_id=?1",[item_id]).map_err(|e|e.to_string())?;tx.execute("DELETE FROM books WHERE id=?1",[item_id]).map_err(|e|e.to_string())?;Ok(())},_=>Err("Unknown trash item type.".into())}}
fn remove_unreferenced_files(s:&Store,c:&Connection,files:Vec<String>){for path in files{let refs:i64=c.query_row("SELECT count(*) FROM attachments WHERE path=?1",[&path],|r|r.get(0)).unwrap_or(1);let file=asset_path(s,&path);if refs==0&&file.starts_with(&s.assets){let _=fs::remove_file(file);}}}
#[tauri::command]
fn permanently_delete_item(s:tauri::State<Store>,item_id:String,kind:String)->Result<(),String>{let mut c=lock(&s)?;let tx=c.transaction().map_err(|e|e.to_string())?;let mut files=Vec::new();hard_delete_item(&tx,&item_id,&kind,&mut files)?;tx.commit().map_err(|e|e.to_string())?;remove_unreferenced_files(&s,&c,files);Ok(())}
#[tauri::command]
fn empty_trash(s:tauri::State<Store>)->Result<(),String>{let mut c=lock(&s)?;let tx=c.transaction().map_err(|e|e.to_string())?;let roots={let mut q=tx.prepare("SELECT id,'book' FROM books WHERE deleted_at IS NOT NULL UNION ALL SELECT ch.id,'chapter' FROM chapters ch JOIN books b ON b.id=ch.book_id LEFT JOIN chapters parent ON parent.id=ch.parent_id WHERE ch.deleted_at IS NOT NULL AND b.deleted_at IS NULL AND (parent.id IS NULL OR parent.deleted_at IS NULL) UNION ALL SELECT n.id,'note' FROM notes n JOIN books b ON b.id=n.book_id LEFT JOIN chapters ch ON ch.id=n.chapter_id WHERE n.deleted_at IS NOT NULL AND b.deleted_at IS NULL AND (ch.id IS NULL OR ch.deleted_at IS NULL)").map_err(|e|e.to_string())?;let rows=q.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(|e|e.to_string())?;rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?};let mut files=Vec::new();for(id,kind)in roots{hard_delete_item(&tx,&id,&kind,&mut files)?;}tx.commit().map_err(|e|e.to_string())?;remove_unreferenced_files(&s,&c,files);Ok(())}

#[tauri::command]
fn list_attachments(s:tauri::State<Store>,note_id:String)->Result<Vec<Attachment>,String>{let c=lock(&s)?;let mut q=c.prepare("SELECT id,note_id,kind,name,path,mime_type,caption,created_at FROM attachments WHERE note_id=?1 ORDER BY created_at").map_err(|e|e.to_string())?;let rows=q.query_map([note_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,Option<String>>(5)?,r.get::<_,Option<String>>(6)?,r.get::<_,String>(7)?))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;Ok(rows.into_iter().map(|(id,note_id,kind,name,path,mime_type,caption,created_at)|Attachment{id,note_id,kind,name,path:asset_path(&s,&path).to_string_lossy().into_owned(),mime_type,caption,created_at}).collect())}
#[tauri::command]
fn import_attachment(s:tauri::State<Store>,note_id:String,source_path:String,kind:String,caption:Option<String>)->Result<Attachment,String>{let source=PathBuf::from(&source_path);if !source.is_file(){return Err("The selected file could not be found.".into())}let name=source.file_name().and_then(|n|n.to_str()).unwrap_or("attachment").to_owned();let extension=source.extension().and_then(|e|e.to_str()).unwrap_or("bin");let asset_id=id();let asset_name=format!("{asset_id}.{extension}");let dest=s.assets.join(&asset_name);fs::copy(&source,&dest).map_err(|e|e.to_string())?;let mime=mime_guess::from_path(&source).first().map(|x|x.to_string());let t=now();let stored=format!("assets/{asset_name}");let path=dest.to_string_lossy().into_owned();let c=lock(&s)?;if let Err(e)=c.execute("INSERT INTO attachments(id,note_id,kind,name,path,mime_type,caption,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![asset_id,note_id,kind,name,stored,mime,caption,t]){let _=fs::remove_file(dest);return Err(e.to_string())}Ok(Attachment{id:asset_id,note_id,kind,name,path,mime_type:mime,caption,created_at:t})}
#[tauri::command]
fn save_recording(s:tauri::State<Store>,note_id:String,name:String,data:Vec<u8>)->Result<Attachment,String>{let asset_id=id();let asset_name=format!("{asset_id}.webm");let path=s.assets.join(&asset_name);fs::write(&path,data).map_err(|e|e.to_string())?;let t=now();let full=path.to_string_lossy().into_owned();let stored=format!("assets/{asset_name}");let mime=Some("audio/webm".to_string());let c=lock(&s)?;c.execute("INSERT INTO attachments(id,note_id,kind,name,path,mime_type,created_at) VALUES(?1,?2,'audio',?3,?4,?5,?6)",params![asset_id,note_id,name,stored,mime,t]).map_err(|e|e.to_string())?;Ok(Attachment{id:asset_id,note_id,kind:"audio".into(),name,path:full,mime_type:mime,caption:None,created_at:t})}
#[tauri::command]
fn read_attachment(s:tauri::State<Store>,attachment_id:String)->Result<String,String>{let c=lock(&s)?;let path:String=c.query_row("SELECT path FROM attachments WHERE id=?1",[attachment_id],|r|r.get(0)).map_err(|e|e.to_string())?;let bytes=fs::read(asset_path(&s,&path)).map_err(|e|e.to_string())?;Ok(base64::Engine::encode(&base64::engine::general_purpose::STANDARD,bytes))}
#[tauri::command]
fn delete_attachment(s:tauri::State<Store>,attachment_id:String)->Result<(),String>{let mut c=lock(&s)?;let tx=c.transaction().map_err(|e|e.to_string())?;let path:Option<String>=tx.query_row("SELECT path FROM attachments WHERE id=?1",[&attachment_id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;tx.execute("DELETE FROM attachments WHERE id=?1",[attachment_id]).map_err(|e|e.to_string())?;tx.commit().map_err(|e|e.to_string())?;if let Some(path)=path{remove_unreferenced_files(&s,&c,vec![path]);}Ok(())}
#[tauri::command]
fn set_attachment_caption(s:tauri::State<Store>,attachment_id:String,caption:String)->Result<(),String>{lock(&s)?.execute("UPDATE attachments SET caption=?1 WHERE id=?2",params![caption,attachment_id]).map(|_|()).map_err(|e|e.to_string())}
#[tauri::command]
fn rename_attachment(s:tauri::State<Store>,attachment_id:String,name:String)->Result<(),String>{lock(&s)?.execute("UPDATE attachments SET name=?1 WHERE id=?2",params![name,attachment_id]).map(|_|()).map_err(|e|e.to_string())}

fn add_file_to_zip(zip:&mut zip::ZipWriter<fs::File>,file:&Path,base:&Path)->Result<(),String>{let name=file.strip_prefix(base).map_err(|e|e.to_string())?.to_string_lossy().replace('\\',"/");zip.start_file(name,zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).map_err(|e|e.to_string())?;let mut input=fs::File::open(file).map_err(|e|e.to_string())?;let mut buffer=[0u8;65536];loop{let n=input.read(&mut buffer).map_err(|e|e.to_string())?;if n==0{break}zip.write_all(&buffer[..n]).map_err(|e|e.to_string())?;}Ok(())}
fn add_asset_tree(zip:&mut zip::ZipWriter<fs::File>,dir:&Path,root:&Path)->Result<(),String>{for entry in fs::read_dir(dir).map_err(|e|e.to_string())?{let path=entry.map_err(|e|e.to_string())?.path();if path.is_dir(){add_asset_tree(zip,&path,root)?}else if path.is_file(){add_file_to_zip(zip,&path,root)?}}Ok(())}
fn html_to_markdown(html:&str)->String{let mut s=html.to_string();for (pattern,replacement) in [(r#"<pre><code class="language-python">"#,"\n```python\n"),(r#"<pre><code class="language-sql">"#,"\n```sql\n"),(r#"<pre><code class="language-javascript">"#,"\n```javascript\n"),(r#"<pre><code class="language-java">"#,"\n```java\n"),(r#"<pre><code class="language-html">"#,"\n```html\n"),(r#"<pre><code class="language-css">"#,"\n```css\n"),("<pre><code>","\n```\n"),("</code></pre>","\n```\n"),("<h1>","\n# "),("</h1>","\n\n"),("<h2>","\n## "),("</h2>","\n\n"),("<h3>","\n### "),("</h3>","\n\n"),("<strong>","**"),("</strong>","**"),("<b>","**"),("</b>","**"),("<em>","*"),("</em>","*"),("<i>","*"),("</i>","*"),("<code>","`"),("</code>","`"),("<br>","\n"),("<br/>","\n"),("</p>","\n\n"),("<p>",""),("<li><p>","- "),("</p></li>","\n"),("<li>","- "),("</li>","\n"),("</ul>","\n"),("<blockquote>","\n> "),("</blockquote>","\n\n"),("<hr>","\n---\n")]{s=s.replace(pattern,replacement)}let tags=regex::Regex::new(r"(?is)<[^>]*>").unwrap();s=tags.replace_all(&s,"").to_string();for (a,b) in [("&nbsp;"," "),("&amp;","&"),("&lt;","<"),("&gt;",">"),("&quot;","\""),("&#39;","'")]{s=s.replace(a,b)}s.trim().to_string()}
#[tauri::command]
fn create_backup(s:tauri::State<Store>,destination:String)->Result<(),String>{let c=lock(&s)?;c.execute_batch("PRAGMA wal_checkpoint(FULL)").map_err(|e|e.to_string())?;let file=fs::File::create(destination).map_err(|e|e.to_string())?;let mut zip=zip::ZipWriter::new(file);zip.start_file("nexnote.sqlite3",zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).map_err(|e|e.to_string())?;let mut db=fs::File::open(&s.db_path).map_err(|e|e.to_string())?;std::io::copy(&mut db,&mut zip).map_err(|e|e.to_string())?;add_asset_tree(&mut zip,&s.assets,&s.assets.parent().unwrap_or(&s.assets))?;zip.finish().map_err(|e|e.to_string())?;Ok(())}

#[tauri::command]
fn restore_backup(s:tauri::State<Store>,source:String)->Result<(),String>{
    let stage=s.db_path.parent().ok_or("database folder missing")?.join(format!(".restore-{}",id()));fs::create_dir_all(stage.join("assets")).map_err(|e|e.to_string())?;
    let result=(||->Result<(),String>{
        let file=fs::File::open(source).map_err(|e|e.to_string())?;let mut archive=zip::ZipArchive::new(file).map_err(|e|e.to_string())?;
        for i in 0..archive.len(){let mut entry=archive.by_index(i).map_err(|e|e.to_string())?;let name=entry.name().replace('\\',"/");let rel=Path::new(&name);if rel.components().any(|c|matches!(c,std::path::Component::ParentDir|std::path::Component::RootDir|std::path::Component::Prefix(_))){return Err("The backup contains an unsafe file path.".into())}if name!="nexnote.sqlite3"&&!name.starts_with("assets/"){continue}if entry.size()>2_000_000_000{return Err("A file in the backup is too large to restore.".into())}let output=stage.join(rel);if entry.is_dir(){fs::create_dir_all(&output).map_err(|e|e.to_string())?;continue}if let Some(parent)=output.parent(){fs::create_dir_all(parent).map_err(|e|e.to_string())?}let mut out=fs::File::create(output).map_err(|e|e.to_string())?;std::io::copy(&mut entry,&mut out).map_err(|e|e.to_string())?;}
        let staged_db=stage.join("nexnote.sqlite3");if !staged_db.is_file(){return Err("This ZIP does not contain a Nexnote backup.".into())}
        {let test=Connection::open(&staged_db).map_err(|e|e.to_string())?;init(&test).map_err(|e|e.to_string())?;let check:String=test.query_row("PRAGMA integrity_check",[],|r|r.get(0)).map_err(|e|e.to_string())?;if check!="ok"{return Err("The backup database failed its integrity check.".into())}}
        let mut guard=lock(&s)?;guard.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").map_err(|e|e.to_string())?;let prior=stage.join("prior.sqlite3");fs::copy(&s.db_path,&prior).map_err(|e|e.to_string())?;let old=std::mem::replace(&mut *guard,Connection::open_in_memory().map_err(|e|e.to_string())?);drop(old);
        let wal=PathBuf::from(format!("{}-wal",s.db_path.to_string_lossy()));let shm=PathBuf::from(format!("{}-shm",s.db_path.to_string_lossy()));let _=fs::remove_file(&wal);let _=fs::remove_file(&shm);
        if let Err(e)=fs::copy(&staged_db,&s.db_path){let _=fs::copy(&prior,&s.db_path);if let Ok(reopened)=Connection::open(&s.db_path){*guard=reopened;}return Err(e.to_string())}
        let restored=Connection::open(&s.db_path).map_err(|e|e.to_string())?;init(&restored).map_err(|e|e.to_string())?;*guard=restored;
        let staged_assets=stage.join("assets");let old_assets=stage.join("prior-assets");if s.assets.exists(){fs::rename(&s.assets,&old_assets).map_err(|e|e.to_string())?;}if let Err(e)=fs::rename(&staged_assets,&s.assets){if old_assets.exists(){let _=fs::rename(&old_assets,&s.assets);}return Err(e.to_string())}Ok(())
    })();let _=fs::remove_dir_all(&stage);result
}

#[tauri::command]
fn export_book_markdown(s:tauri::State<Store>,book_id:String,destination:String)->Result<(),String>{let c=lock(&s)?;let title:String=c.query_row("SELECT title FROM books WHERE id=?1",[&book_id],|r|r.get(0)).map_err(|e|e.to_string())?;let mut out=format!("# {title}\n\n");let mut q=c.prepare("SELECT COALESCE(ch.title,'Unsorted'),n.title,n.content FROM notes n LEFT JOIN chapters ch ON ch.id=n.chapter_id WHERE n.book_id=?1 AND n.deleted_at IS NULL ORDER BY ch.sort_order,n.sort_order").map_err(|e|e.to_string())?;let rows=q.query_map([book_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).map_err(|e|e.to_string())?;for row in rows{let(chapter,note,html)=row.map_err(|e|e.to_string())?;out.push_str(&format!("## {chapter}\n\n### {note}\n\n{}\n\n",html_to_markdown(&html)));}fs::write(destination,out).map_err(|e|e.to_string())}

#[tauri::command]
fn export_note_markdown(s:tauri::State<Store>,note_id:String,destination:String)->Result<(),String>{let c=lock(&s)?;let row:(String,String)=c.query_row("SELECT title,content FROM notes WHERE id=?1 AND deleted_at IS NULL",[&note_id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;fs::write(destination,format!("# {}\n\n{}\n",row.0,html_to_markdown(&row.1))).map_err(|e|e.to_string())}
#[tauri::command]
fn export_attachment(s:tauri::State<Store>,attachment_id:String,destination:String)->Result<(),String>{let c=lock(&s)?;let path:String=c.query_row("SELECT path FROM attachments WHERE id=?1",[&attachment_id],|r|r.get(0)).map_err(|e|e.to_string())?;let source=asset_path(&s,&path);if !source.starts_with(&s.assets)||!source.is_file(){return Err("The attachment file is not available.".into())}fs::copy(source,destination).map(|_|()).map_err(|e|e.to_string())}

pub fn run() {
  tauri::Builder::default().plugin(tauri_plugin_dialog::init()).plugin(tauri_plugin_opener::init()).setup(|app| { let path=database(app.handle()).map_err(std::io::Error::other)?; let assets=path.parent().ok_or(std::io::Error::other("database folder missing"))?.join("assets"); let conn=Connection::open(&path)?; init(&conn)?; app.manage(Store{db:Mutex::new(conn),db_path:path,assets}); Ok(()) })
    .invoke_handler(tauri::generate_handler![list_books,stats,create_book,rename_book,delete_book,toggle_book_favorite,set_book_color,list_chapters,create_chapter,rename_chapter,duplicate_chapter,move_chapter,delete_chapter,reorder_chapter,list_notes,dashboard_notes,create_note,create_note_template,duplicate_note,duplicate_book,move_note,reorder_book,reorder_note,save_note,delete_note,toggle_favorite,set_note_tags,link_note,unlink_note,search_notes,trash_items,restore_item,permanently_delete_item,empty_trash,list_attachments,import_attachment,save_recording,read_attachment,delete_attachment,rename_attachment,set_attachment_caption,create_backup,restore_backup,export_book_markdown,export_note_markdown,export_attachment])
    .run(tauri::generate_context!()).expect("error while running Nexnote");
}

#[cfg(test)]
mod regression_tests {
    use super::*;

    #[test]
    fn migration_indexes_plain_text_and_updates_search() {
        let db=Connection::open_in_memory().unwrap();
        init(&db).unwrap();
        db.execute("INSERT INTO books(id,title,created_at,updated_at) VALUES('b','Journal',datetime('now'),datetime('now'))",[]).unwrap();
        db.execute("INSERT INTO notes(id,book_id,title,content,plain_text,created_at,updated_at) VALUES('n','b','Page','<p>old</p>','old phrase',datetime('now'),datetime('now'))",[]).unwrap();
        let found:i64=db.query_row("SELECT count(*) FROM note_search WHERE note_search MATCH 'phrase'",[],|r|r.get(0)).unwrap();
        assert_eq!(found,1);
        db.execute("UPDATE notes SET plain_text='new searchable phrase' WHERE id='n'",[]).unwrap();
        let old:i64=db.query_row("SELECT count(*) FROM note_search WHERE note_search MATCH 'old'",[],|r|r.get(0)).unwrap();
        let new:i64=db.query_row("SELECT count(*) FROM note_search WHERE note_search MATCH 'searchable'",[],|r|r.get(0)).unwrap();
        assert_eq!(old,0);
        assert_eq!(new,1);
    }

    #[test]
    fn restoring_and_permanently_deleting_book_preserves_shared_assets() {
        let mut db=Connection::open_in_memory().unwrap();
        init(&db).unwrap();
        db.execute_batch("INSERT INTO books(id,title,created_at,updated_at) VALUES('b1','Book',datetime('now'),datetime('now')),('b2','Other',datetime('now'),datetime('now')); INSERT INTO chapters(id,book_id,title,created_at,updated_at) VALUES('c1','b1','Chapter',datetime('now'),datetime('now')); INSERT INTO notes(id,book_id,chapter_id,title,content,plain_text,created_at,updated_at) VALUES('n1','b1','c1','Inside','','',datetime('now'),datetime('now')),('n2','b2',NULL,'Outside','','',datetime('now'),datetime('now')); INSERT INTO attachments(id,note_id,kind,name,path,created_at) VALUES('a1','n1','image','shared.png','assets/shared.png',datetime('now')),('a2','n2','image','shared.png','assets/shared.png',datetime('now'));") .unwrap();
        let stamp="2026-11-08 10:00:00";
        db.execute("UPDATE books SET deleted_at=?1 WHERE id='b1'",[stamp]).unwrap();
        db.execute("UPDATE chapters SET deleted_at=?1 WHERE book_id='b1' AND deleted_at IS NULL",[stamp]).unwrap();
        db.execute("UPDATE notes SET deleted_at=?1 WHERE book_id='b1' AND deleted_at IS NULL",[stamp]).unwrap();
        let tx=db.transaction().unwrap();
        tx.execute("UPDATE books SET deleted_at=NULL WHERE id='b1'",[]).unwrap();
        tx.execute("UPDATE chapters SET deleted_at=NULL WHERE book_id='b1' AND deleted_at=?1",[stamp]).unwrap();
        tx.execute("UPDATE notes SET deleted_at=NULL WHERE book_id='b1' AND deleted_at=?1",[stamp]).unwrap();
        tx.commit().unwrap();
        let restored:i64=db.query_row("SELECT count(*) FROM notes WHERE id='n1' AND deleted_at IS NULL",[],|r|r.get(0)).unwrap();
        assert_eq!(restored,1);

        let tx=db.transaction().unwrap();
        let mut files=Vec::new();
        hard_delete_item(&tx,"b1","book",&mut files).unwrap();
        tx.commit().unwrap();
        let root_notes:i64=db.query_row("SELECT count(*) FROM notes WHERE book_id='b1'",[],|r|r.get(0)).unwrap();
        let root_chapters:i64=db.query_row("SELECT count(*) FROM chapters WHERE book_id='b1'",[],|r|r.get(0)).unwrap();
        let remaining_refs:i64=db.query_row("SELECT count(*) FROM attachments WHERE path='assets/shared.png'",[],|r|r.get(0)).unwrap();
        assert_eq!(root_notes,0);
        assert_eq!(root_chapters,0);
        assert_eq!(remaining_refs,1);
    }

    #[test]
    fn chapter_trash_restores_only_items_deleted_with_that_chapter() {
        let db=Connection::open_in_memory().unwrap();
        init(&db).unwrap();
        db.execute_batch("INSERT INTO books(id,title,created_at,updated_at) VALUES('b','Book',datetime('now'),datetime('now')); INSERT INTO chapters(id,book_id,title,created_at,updated_at) VALUES('c1','b','Parent',datetime('now'),datetime('now')); INSERT INTO chapters(id,book_id,parent_id,title,created_at,updated_at) VALUES('c2','b','c1','Child',datetime('now'),datetime('now')); INSERT INTO notes(id,book_id,chapter_id,title,content,plain_text,created_at,updated_at) VALUES('n1','b','c1','Parent note','','',datetime('now'),datetime('now')),('n2','b','c2','Child note','','',datetime('now'),datetime('now')); UPDATE notes SET deleted_at='earlier' WHERE id='n2';") .unwrap();
        let stamp="chapter-delete";
        db.execute("WITH RECURSIVE tree(id) AS (SELECT id FROM chapters WHERE id='c1' UNION ALL SELECT c.id FROM chapters c JOIN tree t ON c.parent_id=t.id) UPDATE chapters SET deleted_at=?1 WHERE id IN (SELECT id FROM tree) AND deleted_at IS NULL",[stamp]).unwrap();
        db.execute("UPDATE notes SET deleted_at=?1 WHERE chapter_id IN (WITH RECURSIVE tree(id) AS (SELECT id FROM chapters WHERE id='c1' UNION ALL SELECT c.id FROM chapters c JOIN tree t ON c.parent_id=t.id) SELECT id FROM tree) AND deleted_at IS NULL",[stamp]).unwrap();
        db.execute("WITH RECURSIVE tree(id) AS (SELECT id FROM chapters WHERE id='c1' UNION ALL SELECT c.id FROM chapters c JOIN tree t ON c.parent_id=t.id) UPDATE chapters SET deleted_at=NULL WHERE id IN (SELECT id FROM tree) AND deleted_at=?1",[stamp]).unwrap();
        db.execute("UPDATE notes SET deleted_at=NULL WHERE chapter_id IN (WITH RECURSIVE tree(id) AS (SELECT id FROM chapters WHERE id='c1' UNION ALL SELECT c.id FROM chapters c JOIN tree t ON c.parent_id=t.id) SELECT id FROM tree) AND deleted_at=?1",[stamp]).unwrap();
        let active_children:i64=db.query_row("SELECT count(*) FROM chapters WHERE id IN ('c1','c2') AND deleted_at IS NULL",[],|r|r.get(0)).unwrap();
        let active_note:i64=db.query_row("SELECT count(*) FROM notes WHERE id='n1' AND deleted_at IS NULL",[],|r|r.get(0)).unwrap();
        let prior_note:i64=db.query_row("SELECT count(*) FROM notes WHERE id='n2' AND deleted_at='earlier'",[],|r|r.get(0)).unwrap();
        assert_eq!(active_children,2);
        assert_eq!(active_note,1);
        assert_eq!(prior_note,1);
        let mut db=db;
        let tx=db.transaction().unwrap();
        let mut files=Vec::new();
        hard_delete_item(&tx,"c1","chapter",&mut files).unwrap();
        tx.commit().unwrap();
        let removed:i64=db.query_row("SELECT count(*) FROM chapters WHERE id IN ('c1','c2')",[],|r|r.get(0)).unwrap();
        assert_eq!(removed,0);
    }
}
