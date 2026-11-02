'use strict';
const packageSearch = document.querySelector('#package-search');
if (packageSearch) {
    document.querySelector('[data-package-controls]').hidden = false;
    const rows = [...document.querySelectorAll('[data-package-row]')];
    const status = document.querySelector('#status-filter');
    const searchable = rows.map(row => ({ row, text: `${row.cells[0].textContent} ${row.cells[3].textContent}`.toLowerCase() }));
    const filter = () => {
        const query = packageSearch.value.trim().toLowerCase();
        let visible = 0;
        for (const { row, text } of searchable) {
            row.hidden = !text.includes(query) || (status.value !== 'all' && row.dataset.status !== status.value);
            if (!row.hidden) visible++;
        }
        document.querySelector('#no-matches').hidden = visible > 0 || rows.length === 0;
        document.querySelector('#package-count').textContent = `${visible} of ${rows.length} packages`;
    };
    packageSearch.addEventListener('input', filter);
    status.addEventListener('change', filter);
    document.querySelector('#reset-filters').addEventListener('click', () => { packageSearch.value = ''; status.value = 'all'; filter(); packageSearch.focus(); });
}
const logOutput = document.querySelector('#log-output');
if (logOutput) {
    document.querySelector('[data-log-controls]').hidden = false;
    const lines = [...logOutput.querySelectorAll('[data-log-line]')];
    const search = document.querySelector('#log-search');
    search.addEventListener('input', () => {
        const query = search.value.toLowerCase();
        let visible = 0;
        for (const line of lines) { line.hidden = !line.querySelector('code').textContent.toLowerCase().includes(query); if (!line.hidden) visible++; }
        document.querySelector('#log-no-matches').hidden = visible > 0 || lines.length === 0;
        document.querySelector('#log-count').textContent = `${visible} of ${lines.length} lines`;
    });
    document.querySelector('#wrap-lines').addEventListener('change', event => logOutput.classList.toggle('wrap', event.target.checked));
    document.querySelector('#log-bottom').addEventListener('click', () => { logOutput.scrollTop = logOutput.scrollHeight; });
}
document.querySelectorAll('[data-rebuild-form]').forEach(form => form.addEventListener('submit', () => {
    const button = form.querySelector('button'); button.disabled = true; button.textContent = 'Requesting…';
}));
window.addEventListener('pageshow', () => document.querySelectorAll('[data-rebuild-form] button').forEach(button => { button.disabled = false; button.textContent = '↻ Request rebuild'; }));

// Diff content is always escaped by Tera. Highlight with text nodes, never HTML.
function highlightReviewCode(code) {
    const text = code.textContent;
    const pattern = /(#.*$|"(?:[^"\\]|\\.)*"|'[^']*'|\b(?:if|then|else|fi|for|do|done|function|return|source|sha256sums|pkgver|pkgrel|depends|makedepends|prepare|build|package)\b)/g;
    let start = 0;
    for (const match of text.matchAll(pattern)) {
        if (start === 0) code.textContent = '';
        code.append(document.createTextNode(text.slice(start, match.index)));
        const span = document.createElement('span');
        span.className = match[0].startsWith('#') ? 'syntax-comment' : /^["']/.test(match[0]) ? 'syntax-string' : 'syntax-keyword';
        span.textContent = match[0]; code.append(span); start = match.index + match[0].length;
    }
    if (start) code.append(document.createTextNode(text.slice(start)));
}
document.querySelectorAll('[data-syntax]').forEach(highlightReviewCode);
const diffTables = [...document.querySelectorAll('.diff-table')];
function foldDiff(table) {
    const rows = [...table.querySelectorAll('.diff-line')];
    let run = [];
    function flush() {
        if (run.length > 10) {
            const middle = run.slice(3, -3); middle.forEach(row => row.classList.add('hidden-context'));
            const fold = document.createElement('tr'); fold.className = 'diff-fold';
            const cell = document.createElement('td'); cell.colSpan = 4;
            const button = document.createElement('button'); button.type = 'button'; button.textContent = `Show ${middle.length} unchanged lines`;
            button.onclick = () => { middle.forEach(row => row.classList.remove('hidden-context')); fold.remove(); };
            cell.append(button); fold.append(cell); middle[0].before(fold);
        }
        run = [];
    }
    rows.forEach(row => { if (row.classList.contains('equal')) run.push(row); else flush(); }); flush();
}
diffTables.forEach(table => { table.dataset.unified = table.innerHTML; foldDiff(table); });
function diffMode(mode) {
    diffTables.forEach(table => {
        table.innerHTML = table.dataset.unified;
        if (mode === 'split') {
            table.classList.add('split');
            const rows = [...table.querySelectorAll('.diff-line')];
            const body = table.querySelector('tbody');
            body.replaceChildren();
            function pair(left, right, equal = false) {
                const row = document.createElement('tr'); row.className = `diff-line ${equal ? 'equal' : 'changed'}`;
                const cell = (source, selector, className) => {
                    const result = source ? source.querySelector(selector).cloneNode(true) : document.createElement('td');
                    result.className = className; return result;
                };
                row.append(cell(left, '.old-number', 'line-number old-number'), cell(left, '.diff-code', `diff-code ${equal ? '' : 'delete'}`),
                    cell(right, '.new-number', 'line-number new-number'), cell(right, '.diff-code', `diff-code ${equal ? '' : 'insert'}`));
                body.append(row);
            }
            for (let i = 0; i < rows.length;) {
                if (rows[i].classList.contains('equal')) { pair(rows[i], rows[i], true); i++; continue; }
                const deleted = [], inserted = [];
                while (i < rows.length && !rows[i].classList.contains('equal')) {
                    (rows[i].classList.contains('delete') ? deleted : inserted).push(rows[i]); i++;
                }
                for (let j = 0; j < Math.max(deleted.length, inserted.length); j++) pair(deleted[j], inserted[j]);
            }
        } else table.classList.remove('split');
        if (!document.querySelector('#diff-context')?.checked) foldDiff(table);
    });
    document.querySelectorAll('[data-diff-mode]').forEach(button => button.setAttribute('aria-pressed', String(button.dataset.diffMode === mode)));
}
document.querySelectorAll('[data-diff-mode]').forEach(button => button.addEventListener('click', () => diffMode(button.dataset.diffMode)));
document.querySelector('#diff-context')?.addEventListener('change', () => diffMode(document.querySelector('[data-diff-mode][aria-pressed=true]').dataset.diffMode));
