import {useCallback, useState} from "react";
import {apiFetch} from "../../utils/api";

export function useEntityTypes() {
    const [list, setList] = useState([]);
    const [entityTypeData, setEntityTypeData] = useState(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState(null);

    const loadAll = useCallback(() => {
        setLoading(true);

        apiFetch("/api/entity/types")
            .then((response) => {
                setList(response);
                setLoading(false);
            })
            .catch((err) => {
                setError(err);
                setLoading(false);
            });
    }, []);

    const loadOne = useCallback((entityType) => {
        setLoading(true);

        apiFetch("/api/" + entityType + "/schema")
            .then((response) => {
                setEntityTypeData(response);

                setLoading(false);
            })
            .catch((err) => {
                setError(err);
                setLoading(false);
            });
    }, []);

    return { list, loading, error, loadAll, loadOne, entityTypeData };
}
