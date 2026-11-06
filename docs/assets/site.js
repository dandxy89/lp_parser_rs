// copy buttons on code blocks and on the install table's commands
(function () {
  if (!navigator.clipboard) return;
  document.querySelectorAll("pre, #install td code").forEach(function (el) {
    var inPre = el.tagName === "PRE";
    var btn = document.createElement("button");
    btn.type = "button"; btn.className = "copy" + (inPre ? "" : " inline");
    btn.textContent = "copy"; btn.setAttribute("aria-label", "Copy to clipboard");
    btn.addEventListener("click", function () {
      navigator.clipboard.writeText(el.innerText.trim()).then(
        function () { btn.textContent = "copied"; },
        function () { btn.textContent = "failed"; }
      ).then(function () { setTimeout(function () { btn.textContent = "copy"; }, 1500); });
    });
    if (inPre) { var w = document.createElement("div"); w.className = "pre-wrap"; el.replaceWith(w); w.append(el, btn); }
    else el.after(btn);
  });
})();
