import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import './App.css'
import {ToastProvider} from "./components/notification/ToastProvider.jsx";

ReactDOM.createRoot(document.getElementById('root')).render(
    <ToastProvider>
        <App />
    </ToastProvider>
)
