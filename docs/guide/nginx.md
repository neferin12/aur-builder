# Nginx reverse proxies

These HTTP-only virtual hosts assume Nginx runs on the Docker host. Replace the
example domains and enable TLS with Certbot or your certificate-management tooling
before sending credentials or signed URLs over the internet.

If Nginx runs in a container, connect it to the services' Docker network and use
`garage:3900` and `repo:3001` as upstreams instead of host loopback.

## Restrict backend ports

In `docker-compose.yaml`, bind the Garage and repo host ports to loopback:

```yaml
services:
  garage:
    ports:
      - "127.0.0.1:3900:3900"
  repo:
    ports:
      - "127.0.0.1:3001:3001"
```

These are changes to the existing services, not a replacement Compose file. Recreate
the affected containers after editing their port mappings. Keep RabbitMQ's ports
private as well. The web dashboard requires OIDC and an HTTPS reverse proxy; set
`AB_WEB_URL` to its external HTTPS origin and preserve the browser Origin header.

## Garage S3 API

Save as `/etc/nginx/sites-available/garage.conf`:

```nginx
server {
    listen 80;
    listen [::]:80;
    server_name storage.example.com;

    client_max_body_size 5g;
    client_body_timeout 900s;

    # Query strings contain signed credentials.
    access_log off;

    location / {
        # Keep the original request URI; do not append a path or trailing slash.
        proxy_pass http://127.0.0.1:3900;
        proxy_http_version 1.1;
        proxy_set_header Host $http_host;
        proxy_set_header Connection "";
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        proxy_request_buffering off;
        proxy_buffering off;
        proxy_cache off;

        proxy_connect_timeout 30s;
        proxy_send_timeout 900s;
        proxy_read_timeout 900s;
    }
}
```

Preserve Host, path, and query parameters so S3 signatures remain valid. Do not put
an additional login page or Basic-auth challenge in front of S3; signed requests
provide authorization. The upload limit above allows the default 4 GiB bundle
limit plus some headroom; adjust proxy and service limits together.

After enabling TLS, set `.env`:

```dotenv
AB_S3_PUBLIC_ENDPOINT=https://storage.example.com
```

## Pacman repository

Save as `/etc/nginx/sites-available/aur-repo.conf`:

```nginx
server {
    listen 80;
    listen [::]:80;
    server_name repo.example.com;

    location / {
        proxy_pass http://127.0.0.1:3001;
        proxy_http_version 1.1;
        proxy_set_header Host $http_host;
        proxy_set_header Connection "";
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header Authorization $http_authorization;

        # Preserve signed download locations and authentication responses.
        proxy_redirect off;
        proxy_intercept_errors off;
        proxy_cache off;
        proxy_buffering off;

        proxy_connect_timeout 10s;
        proxy_read_timeout 60s;
        proxy_send_timeout 60s;
    }
}
```

The repo service handles Basic authentication. Configure `AB_REPO_USER` and
`AB_REPO_PASSWORD`; an additional Nginx `auth_basic` layer is unnecessary. Downloads
redirect to the separately exposed S3 domain, which clients must also reach.

## Enable the sites

On distributions using `sites-available` / `sites-enabled`:

```sh
sudo ln -s /etc/nginx/sites-available/garage.conf /etc/nginx/sites-enabled/garage.conf
sudo ln -s /etc/nginx/sites-available/aur-repo.conf /etc/nginx/sites-enabled/aur-repo.conf
sudo nginx -t
sudo systemctl reload nginx
```

On other distributions, place the server blocks in a configuration directory
included by your `http` block. Configure TLS, then apply the backend and environment
changes:

```sh
docker compose up -d garage server repo
```

See the official [Garage reverse-proxy guide](https://garagehq.deuxfleurs.fr/documentation/cookbook/reverse-proxy/)
and [Nginx proxy module reference](https://nginx.org/en/docs/http/ngx_http_proxy_module.html).

## Web dashboard

Terminate TLS for the hostname in `AB_WEB_URL` and proxy requests to port 3000:

```nginx
location / {
    proxy_pass http://127.0.0.1:3000;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

Use your existing TLS configuration for this location. The OIDC callback is
`https://your-builder-host/auth/callback`. Cookies require HTTPS; plain HTTP
access to port 3000 cannot establish a session. Do not replace the Origin header.

The UI sends `Cache-Control: no-store`, `Referrer-Policy: no-referrer`, and framing
restrictions on all responses, including OIDC redirects and errors. Preserve these
headers and avoid caching `/auth/*`. Keep proxy access logs for `/auth/callback`
free of query strings, because authorization codes and state are carried there.
