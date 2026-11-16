import { useEffect } from "react";
import type { ReactNode } from "react";
import { EditorContent, useEditor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import CodeBlockLowlight from "@tiptap/extension-code-block-lowlight";
import { Color } from "@tiptap/extension-color";
import Highlight from "@tiptap/extension-highlight";
import Link from "@tiptap/extension-link";
import Placeholder from "@tiptap/extension-placeholder";
import { TextStyle } from "@tiptap/extension-text-style";
import Underline from "@tiptap/extension-underline";
import { Table, TableCell, TableHeader, TableRow } from "@tiptap/extension-table";
import Image from "@tiptap/extension-image";
import { open } from "@tauri-apps/plugin-dialog";
import { createLowlight } from "lowlight";
import python from "highlight.js/lib/languages/python";
import sql from "highlight.js/lib/languages/sql";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import xml from "highlight.js/lib/languages/xml";
import css from "highlight.js/lib/languages/css";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import { Bold, Code2, Copy, Heading1, Heading2, Heading3, Highlighter, ImagePlus, Italic, Link2, List, ListOrdered, Minus, Quote, Table2, Underline as UnderlineIcon } from "lucide-react";
import { api } from "./api";

const lowlight=createLowlight({python,sql,java,javascript,html:xml,xml,css,c,cpp});
const extensions=[StarterKit.configure({codeBlock:false}),CodeBlockLowlight.configure({lowlight,defaultLanguage:"text"}),Table.configure({resizable:true}),TableRow,TableHeader,TableCell,Image.configure({allowBase64:true}),TextStyle,Color,Highlight.configure({multicolor:true}),Underline,Link.configure({openOnClick:false,autolink:true}),Placeholder.configure({placeholder:"Write anything you want to remember…"})];
function normalize(html:string){if(!html)return "<p></p>";if(/<\/?[a-z][^>]*>/i.test(html))return html;return html.split(/\n\s*\n/).map(p=>`<p>${p.split("\n").map(x=>x.replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;")).join("<br>")}</p>`).join("");}

export default function RichEditor({noteId,content,onChange,onBlur,bodyRef,actions,editable=true,onError}:{noteId:string;content:string;onChange:(html:string,text:string)=>void;onBlur:()=>void;bodyRef:(node:HTMLDivElement|null)=>void;actions?:ReactNode;editable?:boolean;onError?:(message:string)=>void}){
  const editor=useEditor({extensions,content:normalize(content),editable,editorProps:{attributes:{class:"tiptap-content"}},onUpdate:({editor:e})=>onChange(e.getHTML(),e.getText({blockSeparator:"\n"})),onBlur});
  useEffect(()=>{if(editor&&editor.storage&&editor.getHTML()!==normalize(content))editor.commands.setContent(normalize(content),{emitUpdate:false});},[editor,noteId]);
  useEffect(()=>{editor?.setEditable(editable);},[editor,editable]);
  if(!editor)return <div className="editor-loading">Preparing your page…</div>;
  const code=()=>editor.isActive("codeBlock")?editor.chain().focus().updateAttributes("codeBlock",{language:"text"}).run():editor.chain().focus().toggleCodeBlock({language:"text"}).run();
  const insertImage=async()=>{try{const path=await open({multiple:false,directory:false,filters:[{name:"Images",extensions:["png","jpg","jpeg","gif","webp","bmp"]}]});if(typeof path!=="string")return;const attachment=await api.importAttachment(noteId,path,"image");const encoded=await api.readAttachment(attachment.id);editor.chain().focus().setImage({src:`data:${attachment.mime_type||"image/png"};base64,${encoded}`,alt:attachment.name}).run();}catch(e){onError?.(String(e));}};
  return <>
    {editable&&<div className="editor-toolbar">
      <div className="toolbar-group">
        <button className={`tool-button ${editor.isActive("heading",{level:1})?"active":""}`} title="Heading 1" onClick={()=>editor.chain().focus().toggleHeading({level:1}).run()}><Heading1 size={17}/></button>
        <button className={`tool-button ${editor.isActive("heading",{level:2})?"active":""}`} title="Heading 2" onClick={()=>editor.chain().focus().toggleHeading({level:2}).run()}><Heading2 size={17}/></button>
        <button className={`tool-button ${editor.isActive("heading",{level:3})?"active":""}`} title="Heading 3" onClick={()=>editor.chain().focus().toggleHeading({level:3}).run()}><Heading3 size={16}/></button>
        <button className={`tool-button strong ${editor.isActive("bold")?"active":""}`} title="Bold" onClick={()=>editor.chain().focus().toggleBold().run()}><Bold size={15}/></button>
        <button className={`tool-button ${editor.isActive("italic")?"active":""}`} title="Italic" onClick={()=>editor.chain().focus().toggleItalic().run()}><Italic size={15}/></button>
        <button className={`tool-button ${editor.isActive("underline")?"active":""}`} title="Underline" onClick={()=>editor.chain().focus().toggleUnderline().run()}><UnderlineIcon size={15}/></button>
        <button className={`tool-button ${editor.isActive("highlight")?"active":""}`} title="Highlight" onClick={()=>editor.chain().focus().toggleHighlight({color:"#f5e7a8"}).run()}><Highlighter size={15}/></button>
        <label className="color-tool" title="Text colour"><input type="color" defaultValue="#587c5c" onChange={e=>editor.chain().focus().setColor(e.target.value).run()}/></label>
        <span className="divider"/>
        <button className={`tool-button ${editor.isActive("bulletList")?"active":""}`} title="Bulleted list" onClick={()=>editor.chain().focus().toggleBulletList().run()}><List size={16}/></button>
        <button className={`tool-button ${editor.isActive("orderedList")?"active":""}`} title="Numbered list" onClick={()=>editor.chain().focus().toggleOrderedList().run()}><ListOrdered size={16}/></button>
        <button className={`tool-button ${editor.isActive("blockquote")?"active":""}`} title="Quote" onClick={()=>editor.chain().focus().toggleBlockquote().run()}><Quote size={15}/></button>
        <button className={`tool-button ${editor.isActive("code")?"active":""}`} title="Inline code" onClick={()=>editor.chain().focus().toggleCode().run()}><Code2 size={15}/></button>
        <button className={`tool-button ${editor.isActive("codeBlock")?"active":""}`} title="Code block" onClick={code}>{"</>"}</button>
        <button className="tool-button" title="Insert table" onClick={()=>editor.chain().focus().insertTable({rows:3,cols:3,withHeaderRow:true}).run()}><Table2 size={15}/></button>
        <button className="tool-button" title="Insert image" onClick={()=>void insertImage()}><ImagePlus size={15}/></button>
        {editor.isActive("codeBlock")&&<button className="tool-button" title="Copy code" onClick={()=>{const text=editor.state.selection.$from.parent.textContent;navigator.clipboard.writeText(text).catch(()=>{});}}><Copy size={14}/></button>}
        <button className="tool-button" title="Insert link" onClick={()=>{const url=window.prompt("Paste a link");if(url)editor.chain().focus().extendMarkRange("link").setLink({href:url}).run();}}><Link2 size={15}/></button>
        <button className="tool-button" title="Horizontal rule" onClick={()=>editor.chain().focus().setHorizontalRule().run()}><Minus size={15}/></button>
      </div>
      <select className="language-picker" aria-label="Code language" value={String(editor.getAttributes("codeBlock").language||"text")} onChange={e=>{const language=e.target.value;if(editor.isActive("codeBlock"))editor.chain().focus().updateAttributes("codeBlock",{language}).run();else editor.chain().focus().toggleCodeBlock({language}).run();}}>
        {["text","python","sql","java","javascript","html","css","c","cpp"].map(l=><option key={l} value={l}>{l.toUpperCase()}</option>)}
      </select>
      <div className="toolbar-right">{actions}</div>
    </div>}
    {!editable&&<div className="reading-actions">{actions}</div>}
    <div className="rich-editor-scroll"><div ref={bodyRef} className="editor-editable"><EditorContent editor={editor}/></div></div>
  </>;
}
