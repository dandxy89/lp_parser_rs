-- Minimal Neovim config to demo lp-lsp in VHS (nvim --clean -u lsp.lua)
vim.filetype.add({ extension = { lp = 'lp' } })
vim.o.number = true
vim.o.signcolumn = 'yes'
vim.o.termguicolors = true
vim.o.updatetime = 300
vim.cmd.colorscheme('habamax')
vim.diagnostic.config({ virtual_text = { prefix = '●' }, underline = true, severity_sort = true })
vim.lsp.config('lp', { cmd = { 'lp-lsp' }, filetypes = { 'lp' }, root_markers = { '.git' } })
vim.lsp.enable('lp')
vim.api.nvim_create_autocmd('LspAttach', { callback = function() vim.lsp.inlay_hint.enable(true) end })
