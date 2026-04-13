(() => {
  const box = document.getElementById("searchBox");
  const results = document.getElementById("searchResults");
  if (!box || !results) return;

  let timer = null;

  function hide() {
    results.hidden = true;
    results.innerHTML = "";
  }

  function show(items) {
    if (!items.length) {
      hide();
      return;
    }
    results.hidden = false;
    const inner = document.createElement("div");
    inner.className = "search-results-inner";
    for (const it of items) {
      const row = document.createElement("div");
      row.className = "search-item";

      const a = document.createElement("a");
      a.href = it.url;
      a.className = "search-title";
      a.textContent = it.title || it.file_path;

      const snip = document.createElement("div");
      snip.className = "search-snippet";
      snip.textContent = it.snippet || "";

      row.appendChild(a);
      row.appendChild(snip);
      inner.appendChild(row);
    }
    results.innerHTML = "";
    results.appendChild(inner);
  }

  async function run(q) {
    const url = "/search?q=" + encodeURIComponent(q);
    const resp = await fetch(url, { headers: { Accept: "application/json" } });
    if (!resp.ok) {
      hide();
      return;
    }
    const data = await resp.json();
    const items = (data && data.results) || [];
    show(items.slice(0, 12));
  }

  box.addEventListener("input", () => {
    const q = box.value.trim();
    if (timer) clearTimeout(timer);
    if (!q) {
      hide();
      return;
    }
    timer = setTimeout(() => {
      run(q).catch(hide);
    }, 120);
  });

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") hide();
  });
})();

