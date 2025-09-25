import http from 'k6/http'
import { check } from 'k6'

http.setResponseCallback(http.expectedStatuses({ min: 200, max: 404 }))

const BASE = __ENV.BASE_URL || 'http://localhost:8089'
const VUS  = parseInt(__ENV.VUS  || '50', 10)
const DURATION = __ENV.DURATION || '30s'

export const options = {
  vus: VUS,
  duration: DURATION,
  thresholds: {
    http_req_failed:   ['rate<0.01'],
    http_req_duration: ['p(95)<100'],
  },
}

function uuid() {
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, c => {
    const r = Math.random() * 16 | 0, v = c === 'x' ? r : (r & 0x3 | 0x8)
    return v.toString(16)
  })
}

export default function () {
  const entity = 'benchmarks'
  const payload = { title: `title-${__VU}-${__ITER}`, value: __ITER }

  const create = http.post(`${BASE}/api/${entity}`, JSON.stringify(payload), {
    headers: { 'Content-Type': 'application/json' },
    tags: { name: 'api_create' }
  })
  check(create, { 'CREATE 200': r => r.status === 200 })

  let created
  try { created = JSON.parse(create.body) } catch (_) { created = null }
  const id = created && created.id

  if (id) {
    const getById = http.get(`${BASE}/api/${entity}/${id}`, { tags: { name: 'api_get_by_id' } })
    check(getById, { 'GET by ID 200': r => r.status === 200 })

    const update = http.put(`${BASE}/api/${entity}/${id}`, JSON.stringify({ title: `updated-${id}`, extra: 123 }), {
      headers: { 'Content-Type': 'application/json' },
      tags: { name: 'api_update' }
    })
    check(update, { 'UPDATE 200': r => r.status === 200 })
  }

  const list = http.get(`${BASE}/api/${entity}`, { tags: { name: 'api_list' } })
  check(list, { 'LIST 200': r => r.status === 200 })

  const filterEq = http.get(`${BASE}/api/${entity}?filter[title][eq]=title-${__VU}-${__ITER}`, { tags: { name: 'api_filter_eq' } })
  check(filterEq, { 'FILTER eq 200': r => r.status === 200 })

  const filterNeq = http.get(`${BASE}/api/${entity}?filter[title][neq]=title-${__VU}-${__ITER}`, { tags: { name: 'api_filter_neq' } })
  check(filterNeq, { 'FILTER neq 200': r => r.status === 200 })

  const filterWildcard = http.get(`${BASE}/api/${entity}?filter[title][eq]=title-*`, { tags: { name: 'api_filter_wildcard' } })
  check(filterWildcard, { 'FILTER wildcard 200': r => r.status === 200 })

  const nested = http.post(`${BASE}/api/nested`, JSON.stringify({ author: { name: "Alice", category: { title: "yep" } } }), {
    headers: { 'Content-Type': 'application/json' },
    tags: { name: 'api_nested_create' }
  })
  check(nested, { 'NESTED CREATE 200': r => r.status === 200 })

  const filterNested = http.get(`${BASE}/api/nested?filter[author.name][eq]=Alice`, { tags: { name: 'api_filter_nested' } })
  check(filterNested, { 'FILTER nested 200': r => r.status === 200 })

  const combined = http.get(`${BASE}/api/${entity}?filter[title][eq]=title-${__VU}-${__ITER}&filter[value][eq]=${__ITER}`, { tags: { name: 'api_filter_combined' } })
  check(combined, { 'FILTER combined 200': r => r.status === 200 })

  const sortAsc = http.get(`${BASE}/api/${entity}?sort[value]=asc`, { tags: { name: 'api_sort_asc' } })
  check(sortAsc, { 'SORT asc 200': r => r.status === 200 })

  const sortDesc = http.get(`${BASE}/api/${entity}?sort[value]=desc`, { tags: { name: 'api_sort_desc' } })
  check(sortDesc, { 'SORT desc 200': r => r.status === 200 })

  if (id) {
    const del = http.del(`${BASE}/api/${entity}/${id}`, null, { tags: { name: 'api_delete' } })
    check(del, { 'DELETE 204': r => r.status === 204 })
  }
}
