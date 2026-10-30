import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

const nodeWebStorageDisabled = process.allowedNodeEnvironmentFlags.has('--experimental-webstorage')
    ? ['--no-experimental-webstorage']
    : []

export default defineConfig({
    plugins: [react()],
    base: '/admin',
    build: {
        outDir: 'dist',
    },
    server: {
        fs: {
            strict: false
        }
    },
    test: {
        environment: 'jsdom',
        setupFiles: ['./tests/setup.js'],
        include: ['tests/**/*.test.{js,jsx}'],
        execArgv: nodeWebStorageDisabled,
    }
})