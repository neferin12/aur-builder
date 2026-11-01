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
