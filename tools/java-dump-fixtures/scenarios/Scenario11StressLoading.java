import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.LinkedList;
import java.util.List;
import java.util.Map;
import java.util.Queue;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.atomic.AtomicLong;
import java.util.concurrent.locks.LockSupport;

/**
 * Stress-loading scenario targeting realistic enterprise
 * application heap patterns.
 *
 * Produces dumps that exercise heap segment extraction and
 * thread resolution at scale:
 * - 200 threads with varying stack depths (10-100 frames)
 * - Millions of small entity objects across collections
 * - Thread-local caches (400k entries) in heavy threads
 *
 * Target dump sizes:
 * - xlarge profile (~16 GB JVM) -> ~12 GB dump
 * - ultra  profile (~28 GB JVM) -> ~20 GB dump
 */
public final class Scenario11StressLoading
        implements HeapScenario {

    @Override
    public String id() {
        return "11";
    }

    @Override
    public String name() {
        return "stress-loading";
    }

    @Override
    public ScenarioHandle setup(ProfileSpec spec)
            throws Exception {
        StressConfig cfg = StressConfig.forProfile(spec);

        AppGraph graph = buildAppGraph(cfg, spec.seed);

        List<StressWorker> workers =
            startWorkers(cfg, spec.seed);

        long totalEntities = graph.entityCount;
        long threadLocalEntities =
            estimateThreadLocalEntities(cfg);

        Map<String, String> metrics = new LinkedHashMap<>();
        metrics.put("scenario",
            "stress-loading (enterprise app simulation)");
        metrics.put("threadCount",
            Integer.toString(workers.size()));
        metrics.put("heavyThreads",
            Integer.toString(cfg.heavyThreadCount));
        metrics.put("mediumThreads",
            Integer.toString(cfg.mediumThreadCount));
        metrics.put("lightThreads",
            Integer.toString(cfg.lightThreadCount));
        metrics.put("backgroundEntities",
            Long.toString(totalEntities));
        metrics.put("threadLocalEntities",
            Long.toString(threadLocalEntities));
        metrics.put("auditQueueSize",
            Integer.toString(cfg.auditEventCount));
        metrics.put("sessionCacheSize",
            Integer.toString(cfg.sessionCount));
        metrics.put("estimatedObjects",
            Long.toString(totalEntities + threadLocalEntities));

        List<Object> roots = new ArrayList<>();
        roots.add(graph);

        AutoCloseable cleanup = () -> {
            for (StressWorker w : workers) {
                w.shutdown();
            }
            for (StressWorker w : workers) {
                w.join(5000);
            }
        };

        return new ScenarioHandle(roots, metrics, cleanup);
    }

    // ── Stress configuration per profile ──────────────

    private static final class StressConfig {
        final int heavyThreadCount;
        final int mediumThreadCount;
        final int lightThreadCount;
        final int heavyStackDepth;
        final int mediumStackDepth;
        final int lightStackDepth;
        final int heavyCacheSize;
        final int customerCount;
        final int productsPerCategory;
        final int categoryCount;
        final int cacheEntryCount;
        final int metricCount;
        final int logEntryCount;
        final int auditEventCount;
        final int sessionCount;

        private StressConfig(
                int heavyThreadCount,
                int mediumThreadCount,
                int lightThreadCount,
                int heavyStackDepth,
                int mediumStackDepth,
                int lightStackDepth,
                int heavyCacheSize,
                int customerCount,
                int productsPerCategory,
                int categoryCount,
                int cacheEntryCount,
                int metricCount,
                int logEntryCount,
                int auditEventCount,
                int sessionCount) {
            this.heavyThreadCount = heavyThreadCount;
            this.mediumThreadCount = mediumThreadCount;
            this.lightThreadCount = lightThreadCount;
            this.heavyStackDepth = heavyStackDepth;
            this.mediumStackDepth = mediumStackDepth;
            this.lightStackDepth = lightStackDepth;
            this.heavyCacheSize = heavyCacheSize;
            this.customerCount = customerCount;
            this.productsPerCategory = productsPerCategory;
            this.categoryCount = categoryCount;
            this.cacheEntryCount = cacheEntryCount;
            this.metricCount = metricCount;
            this.logEntryCount = logEntryCount;
            this.auditEventCount = auditEventCount;
            this.sessionCount = sessionCount;
        }

        int totalThreadCount() {
            return heavyThreadCount
                + mediumThreadCount
                + lightThreadCount;
        }

        static StressConfig forProfile(ProfileSpec spec) {
            switch (spec.name) {
                case "tiny":
                    return new StressConfig(
                        3, 12, 15,
                        30, 15, 10,
                        50_000,
                        20_000, 20, 10,
                        100_000, 10_000, 5_000,
                        10_000, 8_000);
                case "medium":
                    return new StressConfig(
                        5, 20, 35,
                        50, 25, 12,
                        100_000,
                        60_000, 40, 15,
                        300_000, 30_000, 15_000,
                        50_000, 40_000);
                case "large":
                    return new StressConfig(
                        8, 30, 62,
                        70, 35, 15,
                        200_000,
                        500_000, 60, 20,
                        3_000_000, 100_000, 50_000,
                        500_000, 400_000);
                case "xlarge":
                    return new StressConfig(
                        10, 40, 150,
                        100, 50, 20,
                        400_000,
                        3_000_000, 100, 40,
                        10_000_000, 500_000, 200_000,
                        1_500_000, 1_000_000);
                case "ultra":
                    return new StressConfig(
                        10, 40, 150,
                        100, 50, 20,
                        400_000,
                        6_000_000, 120, 50,
                        18_000_000, 800_000, 400_000,
                        3_000_000, 2_000_000);
                default:
                    throw new IllegalArgumentException(
                        "S11 unsupported profile: "
                            + spec.name);
            }
        }
    }

    private static final int HEAVY_CACHE_LEVELS = 5;

    private static long estimateThreadLocalEntities(
            StressConfig cfg) {
        long perFrame = 3;
        long heavyFrames =
            (long) cfg.heavyThreadCount
                * cfg.heavyStackDepth * perFrame;
        long mediumFrames =
            (long) cfg.mediumThreadCount
                * cfg.mediumStackDepth * perFrame;
        long lightFrames =
            (long) cfg.lightThreadCount
                * cfg.lightStackDepth * perFrame;
        long total =
            heavyFrames + mediumFrames + lightFrames;

        if (cfg.heavyCacheSize > 0) {
            total += (long) cfg.heavyThreadCount
                * HEAVY_CACHE_LEVELS
                * cfg.heavyCacheSize
                * 3L;
        }
        return total;
    }

    // ── Background application object graph ───────────

    private static AppGraph buildAppGraph(
            StressConfig cfg, int seed) {
        AppGraph graph = new AppGraph();

        graph.categories =
            buildCategories(cfg.categoryCount, seed);
        graph.products = buildProducts(
            graph.categories,
            cfg.productsPerCategory, seed + 7);
        graph.productIndex =
            indexProducts(graph.products);

        graph.customers = buildCustomers(
            cfg.customerCount, graph.products, seed + 13);
        graph.customerIndex =
            indexCustomers(graph.customers);

        graph.appCache = buildAppCache(
            cfg.cacheEntryCount, graph.products, seed + 19);

        graph.metrics =
            buildMetrics(cfg.metricCount, seed + 23);

        graph.logs =
            buildLogEntries(cfg.logEntryCount, seed + 29);

        graph.configs = buildConfigs(seed + 31);
        graph.permissions =
            buildPermissions(graph.customers, seed + 37);
        graph.auditQueue =
            buildAuditQueue(cfg.auditEventCount, seed + 41);
        graph.sessionCache =
            buildSessionCache(cfg.sessionCount, seed + 43);

        graph.entityCount =
            (long) graph.categories.size()
            + graph.products.size()
            + countCustomerGraph(graph.customers)
            + graph.appCache.size() * 3L
            + graph.metrics.size()
            + graph.logs.size()
            + graph.configs.size()
            + graph.permissions.size()
            + graph.auditQueue.size()
            + graph.sessionCache.size();

        return graph;
    }

    private static List<Category> buildCategories(
            int count, int seed) {
        List<Category> cats = new ArrayList<>(count);
        for (int i = 0; i < count; i++) {
            cats.add(new Category(
                i,
                "category-" + i + "-" + seed));
        }
        return cats;
    }

    private static List<Product> buildProducts(
            List<Category> categories,
            int perCategory, int seed) {
        int total = categories.size() * perCategory;
        List<Product> products = new ArrayList<>(total);
        int id = 0;
        for (Category cat : categories) {
            for (int i = 0; i < perCategory; i++) {
                products.add(new Product(
                    id,
                    "product-" + id + "-" + seed,
                    "SKU-" + id,
                    100L + ((long) id * 7L),
                    cat.id));
                id++;
            }
        }
        return products;
    }

    private static Map<String, Product> indexProducts(
            List<Product> products) {
        Map<String, Product> idx =
            new HashMap<>(products.size() * 2);
        for (Product p : products) {
            idx.put(p.sku, p);
        }
        return idx;
    }

    private static List<Customer> buildCustomers(
            int count, List<Product> products, int seed) {
        List<Customer> customers =
            new ArrayList<>(count);
        AtomicLong orderIdSeq = new AtomicLong(0);
        AtomicLong lineIdSeq = new AtomicLong(0);

        for (int i = 0; i < count; i++) {
            Customer c = new Customer(
                i,
                "customer-" + i + "-" + seed,
                "c" + i + "@app.example.com",
                new Address(
                    (100 + i) + " Main St",
                    "City-" + (i % 500),
                    String.valueOf(10000 + (i % 90000)),
                    "US"));

            int orderCount = 3 + (i % 8);
            c.orders = new ArrayList<>(orderCount);
            for (int j = 0; j < orderCount; j++) {
                long oid = orderIdSeq.getAndIncrement();
                Order o = new Order(
                    oid,
                    (long) c.id,
                    System.currentTimeMillis()
                        - (long) j * 86400000L,
                    j % 4);

                int lineCount = 1 + (j % 5);
                o.lines = new ArrayList<>(lineCount);
                for (int k = 0; k < lineCount; k++) {
                    Product p = products.get(
                        (int) ((oid * 7 + k)
                            % products.size()));
                    o.lines.add(new OrderLine(
                        lineIdSeq.getAndIncrement(),
                        oid,
                        (long) p.id,
                        1 + (k % 10),
                        p.price));
                }
                c.orders.add(o);
            }
            customers.add(c);
        }
        return customers;
    }

    private static Map<Long, Customer> indexCustomers(
            List<Customer> customers) {
        Map<Long, Customer> idx =
            new HashMap<>(customers.size() * 2);
        for (Customer c : customers) {
            idx.put((long) c.id, c);
        }
        return idx;
    }

    private static long countCustomerGraph(
            List<Customer> customers) {
        long count = 0;
        for (Customer c : customers) {
            count += 2; // Customer + Address
            if (c.orders != null) {
                for (Order o : c.orders) {
                    count++; // Order
                    if (o.lines != null) {
                        count += o.lines.size();
                    }
                }
            }
        }
        return count;
    }

    private static ConcurrentHashMap<String, CacheEntry>
            buildAppCache(
                int count,
                List<Product> products,
                int seed) {
        ConcurrentHashMap<String, CacheEntry> cache =
            new ConcurrentHashMap<>(count * 2);
        for (int i = 0; i < count; i++) {
            Product p = products.get(i % products.size());
            cache.put(
                "cache-" + i + "-" + seed,
                new CacheEntry(
                    "cache-" + i + "-" + seed,
                    p,
                    System.currentTimeMillis()
                        - (long) i * 1000L,
                    300_000L + (i % 600_000)));
        }
        return cache;
    }

    private static List<Metric> buildMetrics(
            int count, int seed) {
        List<Metric> metrics = new ArrayList<>(count);
        String[] metricNames = {
            "http.request.duration",
            "http.request.count",
            "jvm.memory.used",
            "jvm.gc.pause",
            "db.query.duration",
            "cache.hit.ratio",
            "thread.pool.active",
            "queue.depth",
            "error.rate",
            "cpu.usage"
        };
        for (int i = 0; i < count; i++) {
            String name =
                metricNames[i % metricNames.length];
            metrics.add(new Metric(
                name,
                (i * 0.01) + (seed % 100),
                System.currentTimeMillis()
                    - (long) i * 100L,
                new String[] {
                    "host:app-" + (i % 10),
                    "env:prod",
                    "service:api"
                }));
        }
        return metrics;
    }

    private static List<LogEntry> buildLogEntries(
            int count, int seed) {
        List<LogEntry> logs = new ArrayList<>(count);
        String[] loggers = {
            "com.app.api.RequestHandler",
            "com.app.db.ConnectionPool",
            "com.app.cache.CacheManager",
            "com.app.auth.SessionManager",
            "com.app.queue.MessageConsumer",
            "com.app.scheduler.TaskRunner"
        };
        for (int i = 0; i < count; i++) {
            logs.add(new LogEntry(
                System.currentTimeMillis()
                    - (long) i * 50L,
                (i % 4),
                "Event #" + i + " seed=" + seed
                    + " processed successfully",
                loggers[i % loggers.length],
                "worker-" + (i % 200)));
        }
        return logs;
    }

    private static List<ConfigProperty> buildConfigs(
            int seed) {
        String[] keys = {
            "db.url", "db.pool.size",
            "db.pool.timeout", "db.pool.max",
            "cache.ttl", "cache.max.size",
            "http.port", "http.max.threads",
            "http.read.timeout", "http.write.timeout",
            "auth.jwt.secret", "auth.session.ttl",
            "queue.url", "queue.prefetch",
            "log.level", "log.format",
            "metrics.interval", "metrics.endpoint",
            "feature.flag.dark.launch",
            "feature.flag.new.ui"
        };
        List<ConfigProperty> configs =
            new ArrayList<>(keys.length);
        for (int i = 0; i < keys.length; i++) {
            configs.add(new ConfigProperty(
                keys[i],
                "value-" + i + "-" + seed,
                i < 10 ? "application.yml" : "env",
                keys[i].contains("secret")));
        }
        return configs;
    }

    private static List<Permission> buildPermissions(
            List<Customer> customers, int seed) {
        String[] resources = {
            "orders", "products", "customers",
            "reports", "admin", "settings"
        };
        String[] actions = {
            "read", "write", "delete", "export"
        };
        int permCount =
            Math.min(customers.size(), 100_000)
                * resources.length;
        List<Permission> perms =
            new ArrayList<>(permCount);
        for (int i = 0;
                i < Math.min(customers.size(), 100_000);
                i++) {
            for (String resource : resources) {
                perms.add(new Permission(
                    resource,
                    actions[i % actions.length],
                    (i + seed) % 3 != 0,
                    System.currentTimeMillis()
                        - (long) i * 3600000L));
            }
        }
        return perms;
    }

    private static LinkedList<AuditEvent> buildAuditQueue(
            int count, int seed) {
        LinkedList<AuditEvent> queue = new LinkedList<>();
        String[] actions = {
            "LOGIN", "LOGOUT", "VIEW_ORDER",
            "PLACE_ORDER", "CANCEL_ORDER",
            "UPDATE_PROFILE", "EXPORT_DATA",
            "ADMIN_ACCESS", "PASSWORD_CHANGE",
            "PERMISSION_GRANT"
        };
        for (int i = 0; i < count; i++) {
            queue.add(new AuditEvent(
                (long) i,
                System.currentTimeMillis()
                    - (long) i * 200L,
                actions[i % actions.length],
                (long) ((i * 7 + seed) % 1_000_000),
                "detail-" + i + "-" + seed));
        }
        return queue;
    }

    private static LinkedHashMap<String, Session>
            buildSessionCache(int count, int seed) {
        LinkedHashMap<String, Session> cache =
            new LinkedHashMap<>(count * 2);
        for (int i = 0; i < count; i++) {
            String sid = "sess-" + i + "-" + seed;
            long now = System.currentTimeMillis();
            cache.put(sid, new Session(
                sid,
                (long) ((i * 13 + seed) % 1_000_000),
                now - (long) i * 60_000L,
                now - (long) (i % 300) * 1_000L,
                i % 10 != 0));
        }
        return cache;
    }

    // ── Worker threads with deep stacks ───────────────

    private static List<StressWorker> startWorkers(
            StressConfig cfg, int seed)
            throws InterruptedException {
        int total = cfg.totalThreadCount();
        CountDownLatch allReady = new CountDownLatch(total);
        List<StressWorker> workers =
            new ArrayList<>(total);

        int threadId = 0;

        for (int i = 0; i < cfg.heavyThreadCount; i++) {
            StressWorker w = new StressWorker(
                "app-request-handler-" + i,
                cfg.heavyStackDepth,
                cfg.heavyCacheSize,
                HEAVY_CACHE_LEVELS,
                allReady,
                seed + threadId);
            workers.add(w);
            threadId++;
        }

        for (int i = 0; i < cfg.mediumThreadCount; i++) {
            StressWorker w = new StressWorker(
                "app-worker-pool-" + i,
                cfg.mediumStackDepth,
                0,
                0,
                allReady,
                seed + threadId);
            workers.add(w);
            threadId++;
        }

        for (int i = 0; i < cfg.lightThreadCount; i++) {
            String name;
            if (i < 30) {
                name = "app-scheduler-" + i;
            } else if (i < 80) {
                name = "app-monitor-" + i;
            } else if (i < 120) {
                name = "app-gc-notifier-" + i;
            } else {
                name = "app-daemon-" + i;
            }
            StressWorker w = new StressWorker(
                name,
                cfg.lightStackDepth,
                0,
                0,
                allReady,
                seed + threadId);
            workers.add(w);
            threadId++;
        }

        for (StressWorker w : workers) {
            w.start();
        }

        allReady.await();
        return workers;
    }

    private static final class StressWorker
            extends Thread {
        private final int targetDepth;
        private final int cacheSize;
        private final int cacheLevels;
        private final CountDownLatch ready;
        private final int seed;
        private volatile boolean running = true;
        private volatile long localEntityCount;

        StressWorker(
                String name,
                int targetDepth,
                int cacheSize,
                int cacheLevels,
                CountDownLatch ready,
                int seed) {
            super(name);
            this.targetDepth = targetDepth;
            this.cacheSize = cacheSize;
            this.cacheLevels = cacheLevels;
            this.ready = ready;
            this.seed = seed;
            setDaemon(true);
        }

        long localEntityCount() {
            return localEntityCount;
        }

        void shutdown() {
            running = false;
            LockSupport.unpark(this);
        }

        @Override
        public void run() {
            long count = deepCall(targetDepth, 0);
            localEntityCount = count;
        }

        /**
         * Recursive descent creating frame-local GC
         * roots at each level. Heavy threads create
         * cache maps at evenly spaced depths.
         *
         * @return count of objects created in this
         *         frame and all children
         */
        private long deepCall(int remaining, long acc) {
            TaskItem localTask = new TaskItem(
                remaining + seed,
                "frame-" + remaining + "-"
                    + getName(),
                remaining % 5,
                remaining % 4,
                seed);

            Connection localConn = new Connection(
                remaining + seed * 31L,
                "db-" + (remaining % 10)
                    + ".internal",
                5432 + (remaining % 100),
                System.currentTimeMillis(),
                remaining % 3 != 0);

            int[] localBuffer =
                new int[] { remaining, seed,
                    remaining * 7 + seed };

            long created = 3;

            if (remaining > 0) {
                Map<String, CacheEntry> frameCache = null;
                if (cacheSize > 0
                        && cacheLevels > 0
                        && remaining
                            % (targetDepth / cacheLevels)
                            == 0) {
                    frameCache =
                        buildFrameCache(
                            cacheSize, remaining);
                    created += (long) cacheSize * 3L;
                }

                long childCount =
                    deepCall(remaining - 1,
                        acc + created);
                created += childCount;

                FixtureRuntime.touch(frameCache);
            } else {
                ready.countDown();
                while (running) {
                    LockSupport.parkNanos(50_000_000L);
                }
            }

            FixtureRuntime.touch(localTask);
            FixtureRuntime.touch(localConn);
            FixtureRuntime.touch(localBuffer);
            return created;
        }

        private Map<String, CacheEntry> buildFrameCache(
                int size, int depth) {
            Map<String, CacheEntry> cache =
                new HashMap<>(size * 2);
            for (int i = 0; i < size; i++) {
                String key = "fc-" + getName()
                    + "-d" + depth + "-" + i;
                cache.put(key, new CacheEntry(
                    key,
                    Integer.valueOf(i),
                    System.currentTimeMillis(),
                    60_000L + i));
            }
            return cache;
        }
    }

    // ── Entity classes (realistic app domain) ─────────

    private static final class AppGraph {
        List<Category> categories;
        List<Product> products;
        Map<String, Product> productIndex;
        List<Customer> customers;
        Map<Long, Customer> customerIndex;
        ConcurrentHashMap<String, CacheEntry> appCache;
        List<Metric> metrics;
        List<LogEntry> logs;
        List<ConfigProperty> configs;
        List<Permission> permissions;
        LinkedList<AuditEvent> auditQueue;
        LinkedHashMap<String, Session> sessionCache;
        long entityCount;
    }

    private static final class Category {
        final int id;
        final String name;

        Category(int id, String name) {
            this.id = id;
            this.name = name;
        }
    }

    private static final class Product {
        final int id;
        final String name;
        final String sku;
        final long price;
        final int categoryId;

        Product(int id, String name, String sku,
                long price, int categoryId) {
            this.id = id;
            this.name = name;
            this.sku = sku;
            this.price = price;
            this.categoryId = categoryId;
        }
    }

    private static final class Customer {
        final int id;
        final String name;
        final String email;
        final Address address;
        List<Order> orders;

        Customer(int id, String name, String email,
                Address address) {
            this.id = id;
            this.name = name;
            this.email = email;
            this.address = address;
        }
    }

    private static final class Address {
        final String street;
        final String city;
        final String zip;
        final String country;

        Address(String street, String city,
                String zip, String country) {
            this.street = street;
            this.city = city;
            this.zip = zip;
            this.country = country;
        }
    }

    private static final class Order {
        final long id;
        final long customerId;
        final long timestamp;
        final int status;
        List<OrderLine> lines;

        Order(long id, long customerId,
                long timestamp, int status) {
            this.id = id;
            this.customerId = customerId;
            this.timestamp = timestamp;
            this.status = status;
        }
    }

    private static final class OrderLine {
        final long id;
        final long orderId;
        final long productId;
        final int quantity;
        final long unitPrice;

        OrderLine(long id, long orderId,
                long productId, int quantity,
                long unitPrice) {
            this.id = id;
            this.orderId = orderId;
            this.productId = productId;
            this.quantity = quantity;
            this.unitPrice = unitPrice;
        }
    }

    private static final class CacheEntry {
        final String key;
        final Object value;
        final long created;
        final long ttl;

        CacheEntry(String key, Object value,
                long created, long ttl) {
            this.key = key;
            this.value = value;
            this.created = created;
            this.ttl = ttl;
        }
    }

    private static final class Metric {
        final String name;
        final double value;
        final long timestamp;
        final String[] tags;

        Metric(String name, double value,
                long timestamp, String[] tags) {
            this.name = name;
            this.value = value;
            this.timestamp = timestamp;
            this.tags = tags;
        }
    }

    private static final class LogEntry {
        final long timestamp;
        final int level;
        final String message;
        final String logger;
        final String threadName;

        LogEntry(long timestamp, int level,
                String message, String logger,
                String threadName) {
            this.timestamp = timestamp;
            this.level = level;
            this.message = message;
            this.logger = logger;
            this.threadName = threadName;
        }
    }

    private static final class ConfigProperty {
        final String key;
        final String value;
        final String source;
        final boolean encrypted;

        ConfigProperty(String key, String value,
                String source, boolean encrypted) {
            this.key = key;
            this.value = value;
            this.source = source;
            this.encrypted = encrypted;
        }
    }

    private static final class Permission {
        final String resource;
        final String action;
        final boolean allowed;
        final long grantedAt;

        Permission(String resource, String action,
                boolean allowed, long grantedAt) {
            this.resource = resource;
            this.action = action;
            this.allowed = allowed;
            this.grantedAt = grantedAt;
        }
    }

    private static final class TaskItem {
        final long id;
        final String description;
        final int priority;
        final int status;
        final long assignee;

        TaskItem(long id, String description,
                int priority, int status,
                long assignee) {
            this.id = id;
            this.description = description;
            this.priority = priority;
            this.status = status;
            this.assignee = assignee;
        }
    }

    private static final class Connection {
        final long id;
        final String endpoint;
        final int port;
        final long createdAt;
        final boolean active;

        Connection(long id, String endpoint,
                int port, long createdAt,
                boolean active) {
            this.id = id;
            this.endpoint = endpoint;
            this.port = port;
            this.createdAt = createdAt;
            this.active = active;
        }
    }

    private static final class AuditEvent {
        final long id;
        final long timestamp;
        final String action;
        final long customerId;
        final String detail;

        AuditEvent(long id, long timestamp,
                String action, long customerId,
                String detail) {
            this.id = id;
            this.timestamp = timestamp;
            this.action = action;
            this.customerId = customerId;
            this.detail = detail;
        }
    }

    private static final class Session {
        final String sessionId;
        final long customerId;
        final long createdAt;
        final long lastAccessAt;
        final boolean active;

        Session(String sessionId, long customerId,
                long createdAt, long lastAccessAt,
                boolean active) {
            this.sessionId = sessionId;
            this.customerId = customerId;
            this.createdAt = createdAt;
            this.lastAccessAt = lastAccessAt;
            this.active = active;
        }
    }
}
