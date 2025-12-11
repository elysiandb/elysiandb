import {useCallback, useState} from "react";
import { ToastContext } from "../../contexts/ToastContext.js";

export function ToastProvider({ children }) {
    const [toasts, setToasts] = useState([]);

    const show = useCallback((type, message) => {
        const id = Math.random().toString(36).slice(2);
        setToasts(t => [...t, { id, type, message }]);
        setTimeout(() => {
            setToasts(t => t.filter(x => x.id !== id));
        }, 3000);
    }, []);

    return (
        <ToastContext.Provider value={{ show }}>
            {children}

            <div className="toast-container">
                {toasts.map(t => (
                    <div key={t.id} className={`toast-item ${t.type}`}>
                        {t.message}
                    </div>
                ))}
            </div>
        </ToastContext.Provider>
    );
}
