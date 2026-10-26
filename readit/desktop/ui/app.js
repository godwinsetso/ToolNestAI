// READIT desktop interface. Every action goes to the Rust backend; the page keeps no data of its own.
"use strict";

const T = window.__TAURI__;
const call = (cmd, args) => T.core.invoke(cmd, args);
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const h = (tag, attrs = {}, ...kids) => {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v == null || v === false) continue;
    if (k.startsWith("on")) e.addEventListener(k.slice(2), v);
    else if (k === "class") e.className = v;
    else if (k === "html") e.innerHTML = v;
    else e.setAttribute(k, v === true ? "" : v);
  }
  for (const c of kids.flat()) if (c != null && c !== false) e.append(c.nodeType ? c : String(c));
  return e;
};

const state = {
  view: "home",
  settings: {},
  now: null,
  doc: null,
  chunks: new Map(), // chunk index -> sections
  outline: [],
  filter: "all",
  query: "",
  packs: [],
};
const CHUNK = 200;

// ------------------------------------------------------------------ helpers

function toast(msg, err = false) {
  const t = h("div", { class: "toast" + (err ? " err" : "") }, h("span", {}, msg), h("button", { "aria-label": "Dismiss", onclick: () => t.remove() }, "✕"));
  $("#toasts").append(t);
  setTimeout(() => t.remove(), err ? 9000 : 4500);
}
async function safe(p) {
  try { return await p; } catch (e) { toast(String(e), true); throw e; }
}
const mb = (b) => (b >= 1e9 ? (b / 1e9).toFixed(1) + " GB" : b >= 1e6 ? (b / 1e6).toFixed(b >= 1e8 ? 0 : 1) + " MB" : Math.max(1, Math.round(b / 1e3)) + " KB");
const kindWord = (k) => ({ page: "Page", slide: "Slide", chapter: "Chapter" })[k] || "";
const formatLabel = (f) => ({ pdf: "PDF", docx: "DOC", doc: "DOC", pptx: "PPT", ppt: "PPT", epub: "EPUB", txt: "TXT", rtf: "RTF", html: "WEB", md: "MD", image: "IMG" })[f] || f.toUpperCase();
function extent(d) {
  if (d.locationKind === "page") return `${d.locationCount} page${d.locationCount === 1 ? "" : "s"}`;
  if (d.locationKind === "slide") return `${d.locationCount} slides`;
  if (d.locationKind === "chapter") return `${d.locationCount} chapters`;
  return `${d.sectionCount} paragraphs`;
}
function ago(sec) {
  if (!sec) return "";
  const d = (Date.now() / 1000 - sec) / 86400;
  if (d < 1) return "today";
  if (d < 2) return "yesterday";
  if (d < 30) return `${Math.floor(d)} days ago`;
  return new Date(sec * 1000).toLocaleDateString();
}
const speedLabel = (s) => (Math.round(s * 100) / 100).toString() + "×";
function voiceName(id) {
  for (const p of state.packs) for (const v of p.voices) if (v.id === id) return `${v.name}, ${v.accent}`;
  return id ? id.split(":")[1] : "Voice";
}

// ------------------------------------------------------------------ navigation

function show(view) {
  state.view = view;
  $$(".view").forEach((v) => v.classList.toggle("active", v.id === "view-" + view));
  $$("nav.side button").forEach((b) => (b.dataset.view === view ? b.setAttribute("aria-current", "page") : b.removeAttribute("aria-current")));
  if (view === "home") renderHome();
  if (view === "library") renderLibrary();
  if (view === "voices") renderVoices();
  if (view === "settings") renderSettings();
  if (view === "listen") scrollToCurrent(false);
}
$$("nav.side button").forEach((b) => b.addEventListener("click", () => show(b.dataset.view)));

// ------------------------------------------------------------------ importing

async function importPaths(paths, openFirst = true) {
  if (!paths?.length) return;
  toast(paths.length === 1 ? "Reading the document…" : `Reading ${paths.length} documents…`);
  const results = await safe(call("import_files", { paths }));
  let first = null;
  let ok = 0;
  for (const r of results) {
    if (r.error) toast(`${r.name}: ${r.error}`, true);
    else {
      ok++;
      first ??= r.document;
      if (r.alreadyInLibrary) toast(`${r.name} is already in your library.`);
      else if (r.document.warnings.length) toast(`${r.name}: ${r.document.warnings[0]}`);
    }
  }
  if (results.length > 1) toast(`Added ${ok} of ${results.length} documents.`);
  if (state.view === "library") renderLibrary();
  if (state.view === "home") renderHome();
  if (first && openFirst) openDoc(first.id, true);
}

async function pickFiles() {
  const picked = await T.dialog.open({
    multiple: true,
    title: "Open documents",
    filters: [
      { name: "Documents", extensions: ["pdf", "docx", "pptx", "epub", "rtf", "txt", "md", "markdown", "html", "htm", "doc", "ppt"] },
      { name: "All files", extensions: ["*"] },
    ],
  });
  if (!picked) return;
  importPaths(Array.isArray(picked) ? picked : [picked]);
}
$("#open-files").addEventListener("click", pickFiles);
$("#lib-import").addEventListener("click", pickFiles);

// Drag and drop anywhere in the window.
T.webview.getCurrentWebview().onDragDropEvent((e) => {
  const drop = $("#drop");
  if (e.payload.type === "over" || e.payload.type === "enter") drop.classList.add("over");
  else drop.classList.remove("over");
  if (e.payload.type === "drop") importPaths(e.payload.paths);
});

// ------------------------------------------------------------------ home and library

function docRow(d, withActions) {
  const row = h("li", {},
    h("div", { class: "fmt", "data-f": d.format, "aria-hidden": "true" }, formatLabel(d.format)),
    h("button", { class: "open", onclick: () => openDoc(d.id, true), "aria-label": `Open ${d.title}` },
      h("div", { class: "t" }, d.title),
      h("div", { class: "m" }, [extent(d), mb(d.sizeBytes), d.lastOpenedAt ? "opened " + ago(d.lastOpenedAt) : "added " + ago(d.importedAt)].join(", "))),
    withActions
      ? h("div", { class: "row-actions" },
          h("button", { class: "icon-btn", "aria-pressed": String(d.favourite), "aria-label": d.favourite ? "Remove from favourites" : "Add to favourites",
            onclick: async () => { await safe(call("set_favourite", { id: d.id, favourite: !d.favourite })); renderLibrary(); },
            html: `<svg viewBox="0 0 24 24"><path fill="${d.favourite ? "currentColor" : "none"}" stroke="currentColor" stroke-width="1.8" d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z"/></svg>` }),
          h("button", { class: "icon-btn", "aria-label": `More for ${d.title}`, onclick: (ev) => docMenu(ev.currentTarget, d),
            html: `<svg viewBox="0 0 24 24"><circle cx="12" cy="5" r="1.8" fill="currentColor"/><circle cx="12" cy="12" r="1.8" fill="currentColor"/><circle cx="12" cy="19" r="1.8" fill="currentColor"/></svg>` }))
      : h("span"));
  return row;
}

async function renderHome() {
  const recent = await call("library", { filter: "recent", query: null });
  const all = recent.length ? recent : await call("library", { filter: "all", query: null });
  const list = $("#recent");
  list.replaceChildren(...all.slice(0, 8).map((d) => docRow(d, false)));
  $("#recent").previousElementSibling.hidden = all.length === 0;
  if (!all.length) list.replaceChildren(h("li", { class: "empty" }, "Your library is empty. Open a document above to start."));
  const n = state.now;
  const c = $("#continue");
  c.replaceChildren();
  if (n?.document) {
    c.append(h("div", { class: "continue" },
      h("div", {},
        h("div", { class: "title" }, n.document.title),
        h("div", { class: "quote" }, n.segment?.text || n.section?.text || "")),
      h("button", { class: "btn primary", onclick: () => { show("listen"); togglePlay(); } }, isPlaying() ? "Pause" : "Continue listening")));
  }
}

async function renderLibrary() {
  const docs = await call("library", { filter: state.filter, query: state.query || null });
  $("#lib-count").textContent = docs.length === 1 ? "1 document" : `${docs.length} documents`;
  const list = $("#library");
  list.replaceChildren(...docs.map((d) => docRow(d, true)));
  if (!docs.length) list.replaceChildren(h("li", { class: "empty" }, state.query ? `Nothing matches "${state.query}".` : state.filter === "favourites" ? "No favourites yet. Use the heart beside a document to add one." : "No documents yet. Use Import, or drop files on the Home page."));
}
$("#lib-search").addEventListener("input", (e) => { state.query = e.target.value; renderLibrary(); });
$$(".chip").forEach((c) => c.addEventListener("click", () => {
  state.filter = c.dataset.filter;
  $$(".chip").forEach((x) => x.setAttribute("aria-pressed", String(x === c)));
  renderLibrary();
}));

function popover(anchor, build, place = "above") {
  closePop();
  const pop = h("div", { class: "pop", role: "dialog" });
  build(pop);
  document.body.append(pop);
  const r = anchor.getBoundingClientRect();
  const pr = pop.getBoundingClientRect();
  let left = Math.min(Math.max(8, r.left + r.width / 2 - pr.width / 2), innerWidth - pr.width - 8);
  let top = place === "above" ? r.top - pr.height - 10 : r.bottom + 8;
  if (top < 8) top = r.bottom + 8;
  if (top + pr.height > innerHeight - 8) top = Math.max(8, innerHeight - pr.height - 8);
  Object.assign(pop.style, { left: left + "px", top: top + "px" });
  (pop.querySelector("input, button, [tabindex]") || pop).focus();
  setTimeout(() => document.addEventListener("mousedown", outside), 0);
  function outside(e) { if (!pop.contains(e.target)) closePop(); }
  pop._cleanup = () => document.removeEventListener("mousedown", outside);
  pop._anchor = anchor;
  return pop;
}
function closePop() {
  const p = $(".pop");
  if (p) { p._cleanup?.(); p.remove(); p._anchor?.focus?.(); }
}

function docMenu(anchor, d) {
  popover(anchor, (pop) => {
    pop.append(
      h("button", { class: "opt", onclick: () => { closePop(); renameDialog(d); } }, "Rename"),
      h("button", { class: "opt", onclick: () => { closePop(); collectionDialog(d); } }, "Add to a collection"),
      h("button", { class: "opt", onclick: () => { closePop(); removeDialog(d); } }, "Remove from READIT"));
  }, "below");
}

function dialog(title, body, actions) {
  const d = h("dialog", {}, h("h3", {}, title), body, h("div", { class: "actions" }, actions));
  document.body.append(d);
  d.addEventListener("close", () => d.remove());
  d.showModal();
  return d;
}
function renameDialog(doc) {
  const input = h("input", { type: "text", value: doc.title, style: "width:100%" });
  const d = dialog("Rename", input, [
    h("button", { class: "btn", onclick: () => d.close() }, "Cancel"),
    h("button", { class: "btn primary", onclick: async () => { await safe(call("rename_document", { id: doc.id, title: input.value.trim() || doc.title })); d.close(); renderLibrary(); } }, "Save"),
  ]);
  input.select();
}
async function collectionDialog(doc) {
  const cols = await call("collections");
  const input = h("input", { type: "text", placeholder: "Collection name", list: "cols", style: "width:100%" });
  const dl = h("datalist", { id: "cols" }, cols.map((c) => h("option", { value: c.name })));
  const d = dialog("Add to a collection", h("div", {}, cols.length ? h("p", { class: "muted" }, "Existing: " + cols.map((c) => `${c.name} (${c.documentCount})`).join(", ")) : null, input, dl), [
    h("button", { class: "btn", onclick: () => d.close() }, "Cancel"),
    h("button", { class: "btn primary", onclick: async () => { if (!input.value.trim()) return; await safe(call("add_to_collection", { name: input.value.trim(), id: doc.id })); d.close(); toast(`Added to ${input.value.trim()}.`); } }, "Add"),
  ]);
}
function removeDialog(doc) {
  const d = dialog(`Remove "${doc.title}"?`, h("p", { class: "muted" }, "READIT deletes its own copy, your bookmarks and notes for it, and any generated audio. The original file on your computer is not touched."), [
    h("button", { class: "btn", onclick: () => d.close() }, "Cancel"),
    h("button", { class: "btn danger", onclick: async () => {
      await safe(call("remove_document", { id: doc.id }));
      d.close();
      if (state.doc?.id === doc.id) { state.doc = null; state.now = await call("now"); renderReader(); }
      toast(`Removed "${doc.title}". Your original file was not changed.`);
      renderLibrary();
    } }, "Remove"),
  ]);
}

// ------------------------------------------------------------------ reader

async function openDoc(id, go) {
  const now = await safe(call("open_document", { id }));
  state.now = now;
  state.doc = now.document;
  state.chunks.clear();
  state.outline = await call("outline", { id });
  renderReader();
  if (go) show("listen");
  updatePlayer();
  if (!now.voiceId && go) toast("No voice is installed yet. Open Voices to download one; it then works offline.");
}

function renderReader() {
  const doc = state.doc;
  $("#reader-empty").hidden = !!doc;
  $("#reader").hidden = !doc;
  $("#reader").style.display = doc ? "flex" : "none";
  if (!doc) return;
  $("#r-title").textContent = doc.title;
  const text = $("#text");
  text.replaceChildren();
  if (doc.sectionCount === 0) {
    text.append(h("div", { class: "ocr-note" },
      h("h2", {}, "No readable text yet"),
      h("p", {}, doc.warnings[0] || "This document has no text layer."),
      h("p", { class: "muted" }, "Reading scanned pages needs text recognition (OCR), which is planned for the next version. This file cannot be read aloud yet.")));
    renderSpine();
    return;
  }
  // One placeholder per chunk of sections; chunks load as they come near the screen.
  const io = new IntersectionObserver((entries) => entries.forEach((e) => e.isIntersecting && loadChunk(+e.target.dataset.chunk)), { root: $("#scroll"), rootMargin: "1200px 0px" });
  const nChunks = Math.ceil(doc.sectionCount / CHUNK);
  for (let i = 0; i < nChunks; i++) {
    const ph = h("div", { class: "chunk", "data-chunk": i });
    ph.style.minHeight = Math.min(CHUNK, doc.sectionCount - i * CHUNK) * 64 + "px";
    text.append(ph);
    io.observe(ph);
  }
  renderSpine();
  scrollToCurrent(false);
}

async function loadChunk(i) {
  if (state.chunks.has(i) || !state.doc) return;
  const docId = state.doc.id;
  state.chunks.set(i, null);
  const secs = await call("sections", { id: docId, from: i * CHUNK, limit: CHUNK });
  if (state.doc?.id !== docId) return;
  state.chunks.set(i, secs);
  const ph = $(`.chunk[data-chunk="${i}"]`);
  if (!ph) return;
  const prevLast = i > 0 ? state.chunks.get(i - 1)?.at(-1) : null;
  const frag = document.createDocumentFragment();
  let lastLoc = prevLast?.location ?? (i === 0 ? -1 : null);
  for (const s of secs) {
    if (state.doc.locationKind !== "none" && lastLoc !== null && s.location !== lastLoc && s.location > 0) {
      frag.append(h("div", { class: "loc-mark", "data-loc": s.location }, `${kindWord(state.doc.locationKind)} ${s.location}`));
    }
    lastLoc = s.location;
    frag.append(sectionEl(s));
  }
  ph.replaceChildren(frag);
  ph.style.minHeight = "";
  highlight();
}

function sectionEl(s) {
  const cls = ["sec"];
  if (s.kind === "heading") cls.push("h", "h" + Math.min(3, Math.max(1, s.level)));
  if (s.kind === "list_item") cls.push(/^\s*(\d{1,3}|[a-z]|[ivx]{1,4})[.)]\s/i.test(s.text) ? "li numbered" : "li");
  if (s.kind === "table_row") cls.push("tr");
  if (s.kind === "code") cls.push("code");
  if (s.kind === "speaker_notes") cls.push("notes");
  if (s.kind === "page_furniture") cls.push("furn");
  const el = h(s.kind === "heading" ? "h" + Math.min(6, Math.max(2, s.level + 1)) : "p", {
    class: cls.join(" "), "data-ord": s.ordinal,
  });
  el.textContent = s.text;
  el.addEventListener("click", () => {
    if (getSelection()?.toString()) return; // selecting text, not jumping
    call("seek_to", { section: s.ordinal, segment: 0 });
    if (!isPlaying()) play();
  });
  return el;
}

let marked = null;
function highlight() {
  const n = state.now;
  if (!n?.section || !state.doc) return;
  const el = $(`.sec[data-ord="${n.section.ordinal}"]`);
  if (marked && marked !== el) {
    marked.classList.remove("current");
    if (marked._orig != null) { marked.textContent = marked._orig; marked._orig = null; }
  }
  if (!el) return;
  el.classList.add("current");
  if (el._orig == null) el._orig = el.textContent;
  const seg = n.segment && n.segment.sectionOrdinal === n.section.ordinal ? n.segment : null;
  if (seg && seg.end <= el._orig.length) {
    const t = el._orig;
    el.replaceChildren(t.slice(0, seg.start), h("mark", { class: "now" }, t.slice(seg.start, seg.end)), t.slice(seg.end));
  } else {
    el.textContent = el._orig;
  }
  marked = el;
}

let follow = true;
let programmaticScroll = 0;
$("#scroll").addEventListener("wheel", () => { follow = false; showFollow(); }, { passive: true });
$("#scroll").addEventListener("keydown", (e) => { if (["PageUp", "PageDown", "Home", "End"].includes(e.key)) { follow = false; showFollow(); } });
function showFollow() {
  if ($("#follow") || !state.now?.section) return;
  const b = h("button", { id: "follow", class: "btn", style: "position:absolute;bottom:110px;left:50%;transform:translateX(-50%);z-index:5",
    onclick: () => { follow = true; b.remove(); scrollToCurrent(true); } }, "Back to the sentence being read");
  $("#view-listen").style.position = "relative";
  $("#view-listen").append(b);
}

async function scrollToCurrent(smooth) {
  const n = state.now;
  if (!n?.section || !state.doc || state.view !== "listen") return;
  const chunk = Math.floor(n.section.ordinal / CHUNK);
  await loadChunk(chunk);
  const el = $(`.sec[data-ord="${n.section.ordinal}"]`);
  if (!el) return;
  const sc = $("#scroll");
  const target = el.querySelector("mark") || el;
  const r = target.getBoundingClientRect();
  const sr = sc.getBoundingClientRect();
  if (!smooth || r.top < sr.top + 60 || r.bottom > sr.bottom - 120) {
    programmaticScroll++;
    sc.scrollTo({ top: sc.scrollTop + r.top - sr.top - sr.height * 0.32, behavior: smooth ? "smooth" : "auto" });
  }
}

// The spine: headings as marks, a dot for "you are here", click to jump.
function renderSpine() {
  const sp = $("#spine");
  sp.replaceChildren();
  const doc = state.doc;
  if (!doc || !doc.sectionCount) return;
  const n = doc.sectionCount;
  for (const o of state.outline) {
    if (o.level > 2) continue;
    sp.append(h("div", { class: "head", style: `top:${(o.ordinal / n) * 100}%` }));
  }
  sp.append(h("div", { class: "done", id: "spine-done" }), h("div", { class: "here", id: "spine-here" }));
  updateSpine();
}
function updateSpine() {
  const doc = state.doc;
  const n = state.now;
  if (!doc || !doc.sectionCount) return;
  const frac = (n?.section?.ordinal ?? 0) / Math.max(1, doc.sectionCount - 1);
  const here = $("#spine-here");
  if (here) here.style.top = frac * 100 + "%";
  const done = $("#spine-done");
  if (done) done.style.height = frac * 100 + "%";
  $("#spine").setAttribute("aria-valuenow", Math.round(frac * 100));
  $("#spine").setAttribute("aria-valuetext", whereText());
}
function spineOrdinal(e) {
  const r = $("#spine").getBoundingClientRect();
  const f = Math.min(1, Math.max(0, (e.clientY - r.top) / r.height));
  return Math.round(f * (state.doc.sectionCount - 1));
}
$("#spine").addEventListener("click", (e) => {
  if (!state.doc?.sectionCount) return;
  const ord = spineOrdinal(e);
  // Snap to a heading if one is close, so a click lands on the start of a section.
  const near = state.outline.reduce((best, o) => (Math.abs(o.ordinal - ord) < Math.abs((best?.ordinal ?? 1e9) - ord) ? o : best), null);
  const target = near && Math.abs(near.ordinal - ord) <= Math.max(3, state.doc.sectionCount * 0.01) ? near.ordinal : ord;
  follow = true;
  navigate("seekSection", target);
});
$("#spine").addEventListener("mousemove", (e) => {
  if (!state.doc?.sectionCount) return;
  const ord = spineOrdinal(e);
  const head = [...state.outline].reverse().find((o) => o.ordinal <= ord);
  const tip = $("#spine-tip");
  tip.textContent = head ? head.text : `${Math.round((ord / state.doc.sectionCount) * 100)}% through`;
  tip.style.display = "block";
  tip.style.left = e.clientX - tip.offsetWidth - 14 + "px";
  tip.style.top = e.clientY - 12 + "px";
});
$("#spine").addEventListener("mouseleave", () => ($("#spine-tip").style.display = "none"));
$("#spine").addEventListener("keydown", (e) => {
  if (e.key === "ArrowUp") { navigate("previousParagraph"); e.preventDefault(); }
  if (e.key === "ArrowDown") { navigate("nextParagraph"); e.preventDefault(); }
});

function whereText() {
  const d = state.doc, s = state.now?.section;
  if (!d || !s) return "";
  if (d.locationKind !== "none" && s.location > 0) return `${kindWord(d.locationKind)} ${s.location} of ${d.locationCount}`;
  return `${Math.round(((s.ordinal + 1) / Math.max(1, d.sectionCount)) * 100)}% through`;
}

// Contents, search and bookmarks open as popovers from the reader header.
$("#r-contents").addEventListener("click", (e) => popover(e.currentTarget, (pop) => {
  pop.append(h("h3", {}, "Contents"));
  if (!state.outline.length) pop.append(h("p", { class: "muted" }, "This document has no headings. Use Search or the bar on the right to move around."));
  for (const o of state.outline) {
    pop.append(h("button", { class: "opt" + (state.now?.section?.ordinal >= o.ordinal ? "" : ""), style: `padding-left:${10 + (o.level - 1) * 14}px`,
      onclick: () => { closePop(); follow = true; navigate("seekSection", o.ordinal); } }, o.text));
  }
}, "below"));

function openSearch(anchor) {
  if (!state.doc) return;
  popover(anchor || $("#r-search"), (pop) => {
    const input = h("input", { type: "search", placeholder: "Find in this document", style: "width:100%" });
    const out = h("div", {});
    let timer;
    input.addEventListener("input", () => {
      clearTimeout(timer);
      timer = setTimeout(async () => {
        const q = input.value.trim();
        if (q.length < 2) return out.replaceChildren();
        const res = await call("search", { id: state.doc.id, query: q });
        out.replaceChildren(h("p", { class: "faint" }, res.length === 1 ? "1 match" : `${res.length} matches`),
          ...res.slice(0, 80).map((s) => h("button", { class: "opt result", onclick: () => { closePop(); follow = true; navigate("seekSection", s.ordinal); } },
            s.location ? h("small", {}, `${kindWord(state.doc.locationKind)} ${s.location}`) : null, s.text.length > 180 ? s.text.slice(0, 180) + "…" : s.text)));
      }, 180);
    });
    pop.append(h("h3", {}, "Search"), input, out);
    pop.style.width = "420px";
  }, "below");
}
$("#r-search").addEventListener("click", (e) => openSearch(e.currentTarget));

$("#r-marks").addEventListener("click", async (e) => {
  const anchor = e.currentTarget;
  const marks = state.doc ? await call("bookmarks", { id: state.doc.id }) : [];
  popover(anchor, (pop) => {
    pop.append(h("h3", {}, "Bookmarks and notes"));
    if (!marks.length) pop.append(h("p", { class: "muted" }, "None yet. Press B while listening to bookmark the sentence being read."));
    for (const m of marks) {
      pop.append(h("div", { style: "display:grid;grid-template-columns:1fr auto;gap:6px;align-items:start;margin-bottom:6px" },
        h("button", { class: "opt result", onclick: () => { closePop(); follow = true; call("seek_to", { section: m.section, segment: m.segment }); } },
          m.label, m.note ? h("small", { style: "color:var(--forest)" }, m.note) : null),
        h("div", {},
          h("button", { class: "btn quiet", onclick: () => { closePop(); noteDialog(m); } }, "Note"),
          h("button", { class: "btn quiet danger", onclick: async () => { await call("delete_bookmark", { id: m.id }); closePop(); toast("Bookmark deleted."); } }, "Delete"))));
    }
    pop.style.width = "440px";
  }, "below");
});
function noteDialog(m) {
  const ta = h("textarea", { rows: 4, style: "width:100%" }, m.note || "");
  const d = dialog("Note", ta, [
    h("button", { class: "btn", onclick: () => d.close() }, "Cancel"),
    h("button", { class: "btn primary", onclick: async () => { await safe(call("update_bookmark", { id: m.id, note: ta.value })); d.close(); toast("Note saved."); } }, "Save"),
  ]);
}

// ------------------------------------------------------------------ player

const isPlaying = () => ["playing", "preparing"].includes(state.now?.state);
const play = () => safe(call("play")).catch(() => {});
const togglePlay = () => safe(call("toggle")).then(refreshNow).catch(() => {});
const navigate = (action, value) => call("navigate", { action, value: value ?? null }).catch((e) => toast(String(e), true));

async function refreshNow() {
  state.now = await call("now");
  updatePlayer();
}

function updatePlayer() {
  const n = state.now || {};
  const playing = isPlaying();
  $("#b-play").setAttribute("aria-label", playing ? "Pause" : "Play");
  $("#b-play").dataset.busy = n.state === "preparing" ? "1" : "0";
  $("#play-icon").innerHTML = playing ? '<path fill="currentColor" d="M7 5h4v14H7zm6 0h4v14h-4z"/>' : '<path fill="currentColor" d="M8 5v14l11-7z"/>';
  $("#p-title").textContent = n.document?.title || "No document open";
  $("#p-sentence").textContent = n.state === "preparing" ? "Preparing speech…" : n.state === "ended" ? "Finished. Press play to start again." : n.segment?.text || n.section?.text || (n.document ? "Ready" : "Open a document to start listening");
  $("#b-speed").textContent = speedLabel(n.speed || 1);
  $("#b-voice").textContent = n.voiceId ? voiceName(n.voiceId) : "Choose a voice";
  const sleeping = n.sleepRemainingSec != null || n.sleepEndOfLocation;
  $("#b-sleep").classList.toggle("on", sleeping);
  $("#b-sleep").textContent = n.sleepEndOfLocation ? `Sleep at end of ${kindWord(n.document?.locationKind).toLowerCase() || "section"}` : n.sleepRemainingSec != null ? `Sleep in ${Math.max(1, Math.ceil(n.sleepRemainingSec / 60))} min` : "Sleep";
  $("#r-where").textContent = whereText();
  updateSpine();
}

$("#b-play").addEventListener("click", togglePlay);
$("#b-prevp").addEventListener("click", () => navigate("previousParagraph"));
$("#b-nextp").addEventListener("click", () => navigate("nextParagraph"));
$("#b-back").addEventListener("click", () => navigate("skip", -(state.settings.skipSeconds || 15)));
$("#b-fwd").addEventListener("click", () => navigate("skip", state.settings.skipSeconds || 15));
$("#b-mark").addEventListener("click", bookmark);
$("#vol").addEventListener("input", (e) => call("set_volume", { volume: +e.target.value }));

async function bookmark() {
  if (!state.doc) return;
  await safe(call("add_bookmark", { note: null }));
  toast("Bookmark added. Find it under Bookmarks above the text.");
}

async function setSpeed(v) {
  const s = await safe(call("set_speed", { speed: v }));
  state.now.speed = s;
  updatePlayer();
}

$("#b-speed").addEventListener("click", (e) => popover(e.currentTarget, (pop) => {
  const val = h("div", { class: "speed-value" }, speedLabel(state.now?.speed || 1));
  const range = h("input", { type: "range", min: "0.5", max: "3", step: "0.05", value: state.now?.speed || 1, "aria-label": "Reading speed" });
  range.addEventListener("input", () => (val.textContent = speedLabel(+range.value)));
  range.addEventListener("change", () => setSpeed(+range.value));
  pop.append(h("h3", {}, "Reading speed"), val, range,
    h("div", { style: "display:flex;gap:6px;margin-top:8px;flex-wrap:wrap" }, [0.75, 1, 1.25, 1.5, 2].map((v) =>
      h("button", { class: "chip", onclick: () => { range.value = v; val.textContent = speedLabel(v); setSpeed(v); } }, speedLabel(v)))),
    h("p", { class: "faint" }, "The sentence being read starts again at the new speed. Shortcuts: [ and ]."));
}));

$("#b-voice").addEventListener("click", async (e) => {
  const anchor = e.currentTarget;
  state.packs = await call("voices");
  popover(anchor, (pop) => {
    pop.append(h("h3", {}, "Voice"));
    const installed = state.packs.filter((p) => p.state.kind === "installed");
    if (!installed.length) pop.append(h("p", { class: "muted" }, "No voice is downloaded yet."));
    for (const p of installed) for (const v of p.voices) {
      pop.append(h("button", { class: "opt" + (state.now?.voiceId === v.id ? " sel" : ""), onclick: async () => {
        closePop();
        toast(`Loading ${v.name}…`);
        await safe(call("select_voice", { id: v.id }));
        refreshNow();
      } }, `${v.name}, ${v.accent} ${v.gender}`));
    }
    pop.append(h("button", { class: "btn quiet", onclick: () => { closePop(); show("voices"); } }, "Manage voices"));
  });
});

$("#b-sleep").addEventListener("click", (e) => popover(e.currentTarget, (pop) => {
  const set = async (minutes, endOfLocation = false) => { closePop(); await call("sleep_timer", { minutes, endOfLocation }); refreshNow(); };
  pop.append(h("h3", {}, "Stop reading after"));
  for (const m of [5, 10, 15, 30, 45, 60]) pop.append(h("button", { class: "opt", onclick: () => set(m) }, `${m} minutes`));
  if (state.doc && state.doc.locationKind !== "none") pop.append(h("button", { class: "opt", onclick: () => set(null, true) }, `The end of this ${kindWord(state.doc.locationKind).toLowerCase()}`));
  pop.append(h("button", { class: "opt", onclick: () => set(null) }, "Turn off"));
}));

// ------------------------------------------------------------------ voices view

async function renderVoices() {
  state.packs = await call("voices");
  const free = await call("free_disk");
  const box = $("#packs");
  box.replaceChildren(...state.packs.map((p) => packEl(p, free)));
}

function fitText(rtf) {
  if (rtf == null) return null;
  if (rtf < 0.5) return ["ok", "Runs smoothly on this computer"];
  if (rtf < 0.9) return ["ok", "Keeps up on this computer"];
  return ["slow", "Too slow here: expect pauses between sentences"];
}

function packEl(p, free) {
  const s = p.state;
  const stateBox = h("div", { class: "state" });
  if (s.kind === "notInstalled" || s.kind === "failed") {
    if (s.kind === "failed") stateBox.append(h("p", { style: "color:var(--danger);margin:0 0 6px" }, s.message));
    stateBox.append(h("button", { class: "btn primary", onclick: () => download(p) }, s.kind === "failed" && s.bytes ? "Resume download" : `Download ${mb(p.downloadBytes)}`));
  } else if (s.kind === "downloading") {
    stateBox.append(h("div", { class: "bar" }, h("i", { style: `width:${(s.bytes / s.total) * 100}%` })), h("div", { class: "faint" }, `${mb(s.bytes)} of ${mb(s.total)}`),
      h("button", { class: "btn quiet", onclick: () => call("pause_download", { pack: p.id }) }, "Pause"));
  } else if (s.kind === "paused") {
    stateBox.append(h("div", { class: "faint" }, `Paused at ${mb(s.bytes)}`), h("button", { class: "btn primary", onclick: () => download(p) }, "Resume"));
  } else if (s.kind === "verifying" || s.kind === "installing") {
    stateBox.append(h("div", { class: "faint" }, s.kind === "verifying" ? "Checking the download…" : "Installing…"));
  } else if (s.kind === "installed") {
    stateBox.append(h("div", { style: "color:var(--forest);font-weight:700" }, "Works offline"),
      s.bundled ? h("div", { class: "faint" }, "Included with READIT") : h("button", { class: "btn quiet danger", onclick: () => removePack(p) }, "Remove"));
  }
  const el = h("article", { class: "pack", "data-pack": p.id },
    h("h2", {}, p.title),
    h("div", { class: "facts" }, h("b", {}, p.quality), `. ${p.voices.length} voice${p.voices.length === 1 ? "" : "s"}, ${mb(p.downloadBytes)} download, about ${mb(p.installedBytes)} on disk. Licence: ${p.licence}. ${p.requirements}`),
    stateBox);
  if (s.kind === "installed") {
    el.append(h("div", { class: "voices" }, p.voices.map((v) => {
      const fit = fitText(p.rtf[v.id]);
      return h("div", { class: "voice" + (state.now?.voiceId === v.id ? " selected" : "") },
        h("div", {}, h("div", { class: "n" }, v.name), h("div", { class: "d" }, `${v.accent}, ${v.gender}`), fit ? h("div", { class: "fit " + fit[0] }, fit[1]) : null),
        h("div", { class: "acts" },
          h("button", { class: "btn quiet", onclick: (ev) => preview(v, ev.currentTarget) }, "Preview"),
          fit ? null : h("button", { class: "btn quiet", onclick: (ev) => testVoice(v, ev.currentTarget) }, "Test"),
          state.now?.voiceId === v.id ? null : h("button", { class: "btn", onclick: async () => { toast(`Loading ${v.name}…`); await safe(call("select_voice", { id: v.id })); await refreshNow(); renderVoices(); } }, "Use")));
    })));
  }
  if (free != null && s.kind === "notInstalled" && free < p.downloadBytes + p.installedBytes + 50e6) {
    stateBox.prepend(h("p", { style: "color:var(--danger);margin:0 0 6px" }, `Needs about ${mb(p.downloadBytes + p.installedBytes)}; ${mb(free)} is free.`));
  }
  return el;
}

async function download(p) {
  await safe(call("download_voice", { pack: p.id }));
  toast(`Downloading ${p.title}. You can keep using READIT.`);
}
function removePack(p) {
  const d = dialog(`Remove ${p.title}?`, h("p", { class: "muted" }, `This frees about ${mb(p.installedBytes)}. You can download it again later. Your documents are not affected.`), [
    h("button", { class: "btn", onclick: () => d.close() }, "Cancel"),
    h("button", { class: "btn danger", onclick: async () => { d.close(); await safe(call("remove_voice_pack", { pack: p.id })); renderVoices(); } }, "Remove"),
  ]);
}
async function preview(v, btn) {
  btn.disabled = true;
  btn.textContent = "Rendering…";
  try {
    if (isPlaying()) await call("pause");
    const url = await call("preview_voice", { id: v.id, text: null });
    const a = $("#preview");
    a.src = url;
    await a.play();
  } catch (e) { toast(String(e), true); }
  btn.disabled = false;
  btn.textContent = "Preview";
}
async function testVoice(v, btn) {
  btn.disabled = true;
  btn.textContent = "Testing…";
  try {
    const rtf = await call("test_voice", { id: v.id });
    const pace = rtf < 1 ? `it renders speech ${(1 / rtf).toFixed(1)} times faster than it speaks` : `it renders speech more slowly than it speaks`;
    toast(`${v.name}: ${fitText(rtf)[1]}. On this computer ${pace}.`);
  } catch (e) { toast(String(e), true); }
  renderVoices();
}

T.event.listen("voice-download", (e) => {
  const { pack, state: s } = e.payload;
  const p = state.packs.find((x) => x.id === pack);
  if (p) p.state = s;
  if (s.kind === "installed") { toast(`${p?.title || "Voice"} is ready and works offline.`); refreshNow(); }
  if (s.kind === "failed") toast(s.message, true);
  if (state.view === "voices") {
    const old = $(`.pack[data-pack="${pack}"]`);
    if (old && p) call("free_disk").then((free) => old.replaceWith(packEl(p, free)));
  }
});

// ------------------------------------------------------------------ settings view

async function renderSettings() {
  const s = (state.settings = await call("get_settings"));
  const profiles = await call("profiles");
  const rules = await call("pronunciations");
  const store = await call("storage");
  const about = await call("about");
  const root = $("#settings");
  const seg = (key, value, options) => h("div", { class: "seg", role: "group" }, options.map(([v, label]) =>
    h("button", { "aria-pressed": String(String(value) === String(v)), onclick: async () => { await call("set_setting", { key, value: String(v) }); applySettings(); renderSettings(); } }, label)));
  const row = (label, help, control) => h("div", { class: "set" }, h("div", {}, h("div", { class: "l" }, label), help ? h("div", { class: "h" }, help) : null), control);

  const active = profiles.find((p) => p.name === s.activeProfile) || profiles[0];
  const pf = profileForm(active, profiles);

  const termIn = h("input", { type: "text", placeholder: "Written as, e.g. Setsoafia" });
  const sayIn = h("input", { type: "text", placeholder: "Say it as, e.g. set-so-AH-fee-ah" });
  const tryIn = h("input", { type: "text", value: "Dr. Mensah paid GH₵ 1,250.50 on 01/03/2026 under KYC rule 4.", style: "width:100%" });
  const spoken = h("div", { class: "spoken" });
  const refreshSpoken = async () => (spoken.textContent = await call("preview_spoken", { text: tryIn.value }));
  tryIn.addEventListener("input", refreshSpoken);
  refreshSpoken();

  root.replaceChildren(
    h("h1", {}, "Settings"),
    h("h2", {}, "Reading profile"),
    h("p", { class: "muted" }, "Each profile keeps its own speed, pauses and choices about what is read. Switch between them as your listening changes."),
    pf,
    h("h2", {}, "Listening"),
    row("Skip forward and back by", "Used by the round arrows and the Left and Right keys.", seg("skip_seconds", s.skipSeconds, [[5, "5 s"], [10, "10 s"], [15, "15 s"], [30, "30 s"], [60, "60 s"]])),
    row("Spell out numbers digit by digit", "Useful for serial numbers, account references and IDs.", seg("spell_mode", s.spellMode ? 1 : 0, [[0, "Off"], [1, "On"]])),
    h("h2", {}, "Pronunciation"),
    h("p", { class: "muted" }, "Teach READIT how to say names, places and terms. Applies to every document."),
    h("div", { class: "toolbar" }, termIn, sayIn, h("button", { class: "btn primary", onclick: async () => {
      if (!termIn.value.trim() || !sayIn.value.trim()) return;
      await safe(call("add_pronunciation", { term: termIn.value.trim(), replacement: sayIn.value.trim(), caseSensitive: false }));
      renderSettings();
    } }, "Add")),
    h("label", { class: "faint" }, "Try a sentence"), tryIn, spoken,
    h("table", { class: "plain", style: "margin-top:12px" }, h("thead", {}, h("tr", {}, h("th", {}, "Written"), h("th", {}, "Said"), h("th", {}))),
      h("tbody", {}, rules.map((r) => h("tr", {}, h("td", {}, r.term), h("td", {}, r.replacement),
        h("td", { style: "text-align:right" }, h("button", { class: "btn quiet danger", onclick: async () => { await call("delete_pronunciation", { id: r.id }); renderSettings(); } }, "Delete")))))),
    h("h2", {}, "Appearance and accessibility"),
    row("Theme", null, seg("theme", s.theme, [["system", "Match system"], ["light", "Light"], ["dark", "Dark"]])),
    row("Text size", "Scales the whole interface and the reading text.", seg("text_scale", s.textScale, [[0.9, "Smaller"], [1, "Standard"], [1.15, "Larger"], [1.35, "Largest"]])),
    row("High contrast", "Pure black and white text and lines.", seg("high_contrast", s.highContrast ? 1 : 0, [[0, "Off"], [1, "On"]])),
    h("p", { class: "faint" }, "Keyboard: Space play or pause, Left and Right skip, Up and Down change paragraph, Page Up and Page Down change page, [ and ] speed, B bookmark, Ctrl+F search, Ctrl+O open."),
    h("h2", {}, "Storage and privacy"),
    h("table", { class: "plain" }, h("tbody", {},
      [["Documents (READIT's own copies)", store.documents], ["Downloaded voices", store.voices], ["Generated audio", store.audioCache], ["Library database", store.database]].map(([l, b]) => h("tr", {}, h("td", {}, l), h("td", { style: "text-align:right" }, mb(b)))),
      store.free != null ? h("tr", {}, h("td", { class: "muted" }, "Free on this disk"), h("td", { style: "text-align:right", class: "muted" }, mb(store.free))) : null)),
    h("p", { class: "muted" }, "READIT needs no account. Your documents, history, bookmarks and voices stay on this computer. The only network use is downloading a voice you choose. Nothing is uploaded and nothing is tracked."),
    h("div", { class: "toolbar" },
      h("button", { class: "btn", onclick: async () => { const n = await safe(call("clear_audio_cache")); toast(`Cleared ${mb(n)} of generated audio. Documents and voices were kept.`); renderSettings(); } }, "Clear generated audio"),
      h("button", { class: "btn danger", onclick: eraseDialog }, "Erase all READIT data")),
    h("h2", {}, "About"),
    h("p", { class: "faint" }, `READIT ${about.version}, ${about.core}. Speech: ${about.engine}. PDF: ${about.pdf}. Fonts: Literata and Atkinson Hyperlegible (SIL Open Font Licence).`),
  );
}

function profileForm(p, profiles) {
  const v = { ...p };
  const field = (label, input) => h("label", {}, label, input);
  const num = (key, min, max, step) => {
    const i = h("input", { type: "number", min, max, step, value: v[key] });
    i.addEventListener("change", () => (v[key] = +i.value));
    return i;
  };
  const chk = (key, label) => {
    const i = h("input", { type: "checkbox" });
    i.checked = !!v[key];
    i.addEventListener("change", () => (v[key] = i.checked));
    return h("label", { class: "check" }, i, label);
  };
  const pick = h("select", { "aria-label": "Profile" }, profiles.map((x) => h("option", { value: x.name, selected: x.name === p.name }, x.name)));
  pick.addEventListener("change", async () => {
    const chosen = profiles.find((x) => x.name === pick.value);
    await safe(call("save_profile", { profile: chosen, activate: true }));
    toast(`Using the ${chosen.name} profile.`);
    refreshNow();
    renderSettings();
  });
  return h("div", {},
    h("div", { class: "toolbar" }, pick, h("span", { class: "faint" }, "Active profile")),
    h("div", { class: "profile-grid" },
      field("Speed (0.5 to 3)", num("speed", 0.5, 3, 0.05)),
      field("Pause after paragraphs (ms)", num("paragraphPauseMs", 0, 3000, 50)),
      field("Pause after headings (ms)", num("headingPauseMs", 0, 4000, 50)),
      h("div", {}),
      chk("skipHeadersFooters", "Skip page headers, footers and page numbers"),
      chk("readSlideContent", "Presentations: read slide content"),
      chk("readSpeakerNotes", "Presentations: read speaker notes"),
      h("label", { class: "check" }, (() => { const i = h("input", { type: "checkbox" }); i.addEventListener("change", () => (v.voiceId = i.checked ? state.now?.voiceId : p.voiceId)); return i; })(), "Use the current voice for this profile")),
    h("div", { class: "toolbar", style: "margin-top:12px" }, h("button", { class: "btn primary", onclick: async () => {
      if (!v.readSlideContent && !v.readSpeakerNotes) v.readSlideContent = true;
      await safe(call("save_profile", { profile: v, activate: true }));
      toast(`Saved the ${v.name} profile.`);
      refreshNow();
    } }, "Save profile")));
}

function eraseDialog() {
  const d = dialog("Erase everything?", h("p", { class: "muted" }, "This deletes READIT's library, its copies of your documents, bookmarks, notes, history, settings, pronunciations and downloaded voices from this computer. Original files elsewhere are not touched. This cannot be undone."), [
    h("button", { class: "btn", onclick: () => d.close() }, "Cancel"),
    h("button", { class: "btn danger", onclick: async () => { await safe(call("erase_all")); d.close(); state.doc = null; state.now = await call("now"); renderReader(); updatePlayer(); applySettings(); toast("All READIT data on this computer has been erased."); show("home"); } }, "Erase"),
  ]);
}

async function applySettings() {
  const s = (state.settings = await call("get_settings"));
  const dark = s.theme === "dark" || (s.theme === "system" && matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  document.documentElement.dataset.contrast = s.highContrast ? "high" : "normal";
  document.documentElement.style.setProperty("--scale", s.textScale);
  $("#vol").value = s.volume;
  $("#skip-a").textContent = $("#skip-b").textContent = s.skipSeconds;
  $("#b-back").setAttribute("aria-label", `Back ${s.skipSeconds} seconds`);
  $("#b-fwd").setAttribute("aria-label", `Forward ${s.skipSeconds} seconds`);
  return s;
}
matchMedia("(prefers-color-scheme: dark)").addEventListener("change", applySettings);

// ------------------------------------------------------------------ events and keys

T.event.listen("player-state", (e) => {
  if (!state.now) return;
  state.now.state = e.payload;
  updatePlayer();
});
T.event.listen("player-segment", (e) => {
  if (!state.now) return;
  state.now.section = e.payload.section;
  state.now.segment = e.payload.segment;
  updatePlayer();
  highlight();
  if (follow) scrollToCurrent(true);
});
T.event.listen("player-error", (e) => toast(e.payload, true));
T.event.listen("player-message", (e) => { toast(e.payload); refreshNow(); });

document.addEventListener("keydown", (e) => {
  const typing = /INPUT|TEXTAREA|SELECT/.test(document.activeElement?.tagName) || document.activeElement?.isContentEditable;
  if (e.key === "Escape") return closePop();
  if (e.altKey && /^[1-5]$/.test(e.key)) { show(["home", "library", "listen", "voices", "settings"][+e.key - 1]); e.preventDefault(); return; }
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "o") { pickFiles(); e.preventDefault(); return; }
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") { if (state.doc) { show("listen"); openSearch(); } e.preventDefault(); return; }
  if (typing || e.ctrlKey || e.metaKey || e.altKey || $("dialog[open]")) return;
  const skip = state.settings.skipSeconds || 15;
  const map = {
    " ": togglePlay,
    ArrowLeft: () => navigate("skip", -skip),
    ArrowRight: () => navigate("skip", skip),
    ArrowUp: () => navigate("previousParagraph"),
    ArrowDown: () => navigate("nextParagraph"),
    PageUp: () => navigate("previousLocation"),
    PageDown: () => navigate("nextLocation"),
    "[": () => setSpeed(Math.max(0.5, (state.now?.speed || 1) - 0.1)),
    "]": () => setSpeed(Math.min(3, (state.now?.speed || 1) + 0.1)),
    b: bookmark,
    B: bookmark,
  };
  if (document.activeElement?.tagName === "BUTTON" && e.key === " ") return; // let buttons activate
  const f = map[e.key];
  if (f && state.doc) { f(); e.preventDefault(); follow = true; $("#follow")?.remove(); }
});

// Keep the sleep-timer countdown fresh.
setInterval(() => { if (state.now?.sleepRemainingSec != null) refreshNow(); }, 30000);

// ------------------------------------------------------------------ first run

async function welcome() {
  state.packs = await call("voices");
  const overlay = h("div", { style: "position:fixed;inset:0;background:var(--paper);z-index:40;overflow:auto" });
  const installed = state.packs.filter((p) => p.state.kind === "installed");
  const step2 = () => {
    overlay.replaceChildren(h("div", { class: "welcome" },
      h("p", { class: "big" }, "Choose how READIT sounds"),
      h("p", {}, installed.some((p) => p.engine === "kokoro")
        ? "A natural voice is already installed, so you can start now. The voices below are optional extras; each works offline once downloaded."
        : installed.length
          ? `${installed[0].title} is installed, so you can start now. For the most natural sound, download Kokoro too; it works offline afterwards.`
          : "Download one voice to start. After that, READIT works without internet."),
      ...state.packs.filter((p) => p.state.kind !== "installed").slice(0, 2).map((p) => h("div", { class: "choice" },
        h("div", {}, h("strong", {}, p.title), h("div", { class: "faint" }, `${p.quality}. ${mb(p.downloadBytes)}. ${p.requirements}`)),
        h("button", { class: "btn", onclick: (ev) => { download(p); ev.currentTarget.disabled = true; ev.currentTarget.textContent = "Downloading"; } }, "Download"))),
      h("div", { style: "margin-top:24px" }, h("button", { class: "btn primary", onclick: async () => { await call("set_setting", { key: "onboarding_done", value: "1" }); overlay.remove(); } }, "Start reading"))));
  };
  overlay.append(h("div", { class: "welcome" },
    h("p", { class: "big" }, "Listen to your documents."),
    h("p", {}, "Open a PDF, Word file, presentation or e-book and READIT reads it aloud, highlighting each sentence. Pause, skip, change speed and pick up exactly where you left off."),
    h("p", {}, "There is no account. Your documents and listening history stay on this computer, and nothing is uploaded."),
    h("button", { class: "btn primary", onclick: step2 }, "Continue")));
  document.body.append(overlay);
}

// ------------------------------------------------------------------ start

(async function start() {
  const s = await applySettings();
  state.packs = await call("voices");
  state.now = await call("now");
  updatePlayer();
  renderHome();
  if (!s.onboardingDone) welcome();
  const files = await call("startup_files");
  if (files.length) importPaths(files);
  else {
    // Reopen the last document at its saved sentence, ready to continue (not playing).
    const recent = await call("library", { filter: "recent", query: null });
    if (recent.length && !state.doc) {
      await openDoc(recent[0].id, false);
      renderHome();
    }
  }
})();
