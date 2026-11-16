import { invoke } from "@tauri-apps/api/core";

export type Book = { id:string; title:string; description:string; color:string; created_at:string; updated_at:string; favorite:boolean; chapter_count:number; note_count:number };
export type Chapter = { id:string; book_id:string; title:string; sort_order:number; note_count:number };
export type Note = { id:string; book_id:string; chapter_id:string|null; title:string; content:string; plain_text:string; updated_at:string; favorite:boolean; tags:string[] };
export type SearchResult = { note_id:string; title:string; snippet:string; book_id:string; book_title:string; chapter_title:string };
export type Stats = { books:number; chapters:number; notes:number; attachments:number; recordings:number };
export type Attachment = { id:string;note_id:string;kind:string;name:string;path:string;mime_type:string|null;caption:string|null;created_at:string };
export type TrashItem = { id:string;title:string;kind:"book"|"chapter"|"note";deleted_at:string;location:string };

export const api = {
  books:()=>invoke<Book[]>("list_books"), stats:()=>invoke<Stats>("stats"), dashboardNotes:()=>invoke<Note[]>("dashboard_notes"),
  createBook:(title:string,description:string,color:string)=>invoke<string>("create_book",{title,description,color}),
  renameBook:(bookId:string,title:string,description:string)=>invoke<void>("rename_book",{bookId,title,description}), reorderBook:(bookId:string,direction:number)=>invoke<void>("reorder_book",{bookId,direction}),
  favoriteBook:(bookId:string)=>invoke<void>("toggle_book_favorite",{bookId}), setBookColor:(bookId:string,color:string)=>invoke<void>("set_book_color",{bookId,color}),
  duplicateBook:(bookId:string)=>invoke<string>("duplicate_book",{bookId}),
  deleteBook:(bookId:string)=>invoke<void>("delete_book",{bookId}),
  chapters:(bookId:string)=>invoke<Chapter[]>("list_chapters",{bookId}),
  createChapter:(bookId:string,title:string)=>invoke<string>("create_chapter",{bookId,title}), renameChapter:(chapterId:string,title:string)=>invoke<void>("rename_chapter",{chapterId,title}), reorderChapter:(chapterId:string,direction:number)=>invoke<void>("reorder_chapter",{chapterId,direction}), duplicateChapter:(chapterId:string)=>invoke<string>("duplicate_chapter",{chapterId}), moveChapter:(chapterId:string,bookId:string)=>invoke<void>("move_chapter",{chapterId,bookId}), deleteChapter:(chapterId:string)=>invoke<void>("delete_chapter",{chapterId}),
  notes:(bookId:string)=>invoke<Note[]>("list_notes",{bookId}),
  createNote:(bookId:string,chapterId:string|null)=>invoke<string>("create_note",{bookId,chapterId}),
  createTemplate:(bookId:string,chapterId:string|null,template:string)=>invoke<string>("create_note_template",{bookId,chapterId,template}),
  saveNote:(noteId:string,title:string,content:string,plainText:string)=>invoke<void>("save_note",{noteId,title,content,plainText}),
  deleteNote:(noteId:string)=>invoke<void>("delete_note",{noteId}), favorite:(noteId:string)=>invoke<void>("toggle_favorite",{noteId}),
  duplicateNote:(noteId:string)=>invoke<string>("duplicate_note",{noteId}), moveNote:(noteId:string,bookId:string,chapterId:string|null)=>invoke<void>("move_note",{noteId,bookId,chapterId}), reorderNote:(noteId:string,direction:number)=>invoke<void>("reorder_note",{noteId,direction}),
  setTags:(noteId:string,tags:string[])=>invoke<void>("set_note_tags",{noteId,tags}), linkNote:(sourceNoteId:string,targetNoteId:string)=>invoke<void>("link_note",{sourceNoteId,targetNoteId}), unlinkNote:(sourceNoteId:string,targetNoteId:string)=>invoke<void>("unlink_note",{sourceNoteId,targetNoteId}),
  search:(query:string,bookId:string|null=null,fromDate:string|null=null,toDate:string|null=null)=>invoke<SearchResult[]>("search_notes",{query,bookId,fromDate,toDate}),
  trash:()=>invoke<TrashItem[]>("trash_items"), restore:(itemId:string,kind:string)=>invoke<void>("restore_item",{itemId,kind}), permanentlyDelete:(itemId:string,kind:string)=>invoke<void>("permanently_delete_item",{itemId,kind}), emptyTrash:()=>invoke<void>("empty_trash"), restoreBackup:(source:string)=>invoke<void>("restore_backup",{source}),
  attachments:(noteId:string)=>invoke<Attachment[]>("list_attachments",{noteId}), importAttachment:(noteId:string,sourcePath:string,kind:string,caption:string|null=null)=>invoke<Attachment>("import_attachment",{noteId,sourcePath,kind,caption}), saveRecording:(noteId:string,name:string,data:number[])=>invoke<Attachment>("save_recording",{noteId,name,data}), readAttachment:(attachmentId:string)=>invoke<string>("read_attachment",{attachmentId}), deleteAttachment:(attachmentId:string)=>invoke<void>("delete_attachment",{attachmentId}), renameAttachment:(attachmentId:string,name:string)=>invoke<void>("rename_attachment",{attachmentId,name}), setAttachmentCaption:(attachmentId:string,caption:string)=>invoke<void>("set_attachment_caption",{attachmentId,caption}), exportAttachment:(attachmentId:string,destination:string)=>invoke<void>("export_attachment",{attachmentId,destination}),
  backup:(destination:string)=>invoke<void>("create_backup",{destination}), exportBook:(bookId:string,destination:string)=>invoke<void>("export_book_markdown",{bookId,destination}), exportNote:(noteId:string,destination:string)=>invoke<void>("export_note_markdown",{noteId,destination})
};
