import http from 'k6/http'
import { check } from 'k6'

http.setResponseCallback(http.expectedStatuses({ min: 200, max: 404 }))

const BASE = __ENV.BASE_URL || 'http://localhost:8089'
const VUS  = parseInt(__ENV.VUS  || '200', 10)
const DURATION = __ENV.DURATION || '30s'

export const options = {
  vus: VUS,
  duration: DURATION,
  thresholds: {
    http_req_failed:   ['rate<0.01'],
    http_req_duration: ['p(95)<25'],
  },
}

function keyForThisIter() {
  return `bench-${__VU}-${__ITER}`
}

export default function () {
  const key = keyForThisIter()
  const val = `value=${__ITER}`
  const useTTL = (__ITER % 2) === 0
  const urlPut = useTTL ? `${BASE}/kv/${key}?ttl=50` : `${BASE}/kv/${key}`

  const put = http.put(
    urlPut,
    val,
    { headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, tags: { name: 'kv_put' } }
  );
  check(put, { 'PUT 204': (r) => r.status === 204 })

  const get = http.get(`${BASE}/kv/${key}`, { tags: { name: 'kv_get' } })
  check(get, {
    'GET 200': (r) => {
      if (r.status !== 200) return false
      try {
        const body = JSON.parse(r.body)
        return body && body.key === key && body.value === val;404
      } catch (_) {
        return false
      }
    },
  })

  const missKey = `${key}-missing`;
  const mget = http.get(`${BASE}/kv/mget?keys=${encodeURIComponent(key)},${encodeURIComponent(missKey)}`, { tags: { name: 'kv_mget' }, expectedStatuses: [200] })
  check(mget, {
    'MGET 200': (r) => {
      if (r.status !== 200) return false
      try {
        const arr = JSON.parse(r.body);
        if (!Array.isArray(arr) || arr.length !== 2) return false
        const a = arr[0], b = arr[1];
        return a.key === key && a.value === val && b.key === missKey && b.value === null
      } catch (_) {
        return false
      }
    },
  })

  const del = http.del(`${BASE}/kv/${key}`, null, { tags: { name: 'kv_del' } })
  check(del, { 'DEL 204': (r) => r.status === 204 })

  const getAfterDel = http.get(`${BASE}/kv/${key}`, { tags: { name: 'kv_get_after_del' } })
  check(getAfterDel, {
    'GET after DEL 404': (r) => {
      if (r.status !== 404) return false
      try {
        const body = JSON.parse(r.body);
        return body && body.key === key && body.value === null
      } catch (_) {
        return false
      }
    },
  })
}
