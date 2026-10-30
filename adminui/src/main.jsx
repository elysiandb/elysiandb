import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import './App.css'
import {ToastProvider} from "./components/notification/ToastProvider.jsx";
import {bindDocumentLanguage} from "./i18n/document.js";

bindDocumentLanguage();

ReactDOM.createRoot(document.getElementById('root')).render(
    <ToastProvider>
        <App />
    </ToastProvider>
)
