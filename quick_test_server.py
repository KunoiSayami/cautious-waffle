import asyncio

import aiohttp
from aiohttp import web


async def resp(request: web.Request):
    print(request.headers)
    return web.Response(status=200)


async def main():
    app = web.Application()
    app.add_routes([web.get('/', resp)])
    runner = web.AppRunner(app)
    await runner.setup()
    site = web.TCPSite(runner, port=23366)
    await site.start()
    while True:
        try:
            await asyncio.sleep(1)
        except KeyboardInterrupt:
            break
    await site.stop()


if __name__ == '__main__':
    asyncio.run(main())
