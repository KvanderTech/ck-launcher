#include "ui.h"
#include <QBuffer>
#include <QImageReader>

#include <QPainterPath>
#include <cmath>

QIcon glyph(const QString &name, const QColor &color, int size) {
    QPixmap pix(size * 2, size * 2);
    pix.fill(Qt::transparent);
    QPainter p(&pix);
    p.setRenderHint(QPainter::Antialiasing);
    p.scale(size / 12.0, size / 12.0);
    p.setPen(QPen(color, 1.75, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
    p.setBrush(Qt::NoBrush);
    auto line = [&](qreal x, qreal y, qreal a, qreal b) {
        p.drawLine(QPointF(x, y), QPointF(a, b));
    };
    if (name == s("home")) {
        p.drawPolyline(QPolygonF(QVector<QPointF>{QPointF(3, 11), {12, 3}, {21, 11}}));
        p.drawRect(QRectF(6, 10, 12, 11));
        p.drawRect(QRectF(10, 14, 4, 7));
    } else if (name == s("library")) {
        p.drawRoundedRect(QRectF(3, 4, 4, 17), 1, 1);
        p.drawRoundedRect(QRectF(9, 4, 4, 17), 1, 1);
        p.save();
        p.translate(16, 4);
        p.rotate(-10);
        p.drawRoundedRect(QRectF(0, 0, 4, 17), 1, 1);
        p.restore();
    } else if (name == s("grid")) {
        for (int y : {4, 14})
            for (int x : {4, 14})
                p.drawRoundedRect(QRectF(x, y, 6, 6), 1.3, 1.3);
    } else if (name == s("shirt")) {
        p.drawPolygon(QPolygonF(QVector<QPointF>{{8, 3},
                                                 {9, 5},
                                                 {15, 5},
                                                 {16, 3},
                                                 {22, 7},
                                                 {19, 12},
                                                 {17, 10},
                                                 {17, 21},
                                                 {7, 21},
                                                 {7, 10},
                                                 {5, 12},
                                                 {2, 7}}));
    } else if (name == s("settings")) {
        line(3, 6, 21, 6);
        line(3, 12, 21, 12);
        line(3, 18, 21, 18);
        p.setBrush(QColor(8, 20, 34));
        p.drawEllipse(QPointF(15, 6), 2.2, 2.2);
        p.drawEllipse(QPointF(8, 12), 2.2, 2.2);
        p.drawEllipse(QPointF(15, 18), 2.2, 2.2);
    } else if (name == s("close")) {
        line(7, 7, 17, 17);
        line(17, 7, 7, 17);
    } else if (name == s("minus")) {
        line(6, 12, 18, 12);
    } else if (name == s("maximize")) {
        p.drawRoundedRect(QRectF(7, 7, 10, 10), 1, 1);
    } else if (name == s("plus")) {
        line(5, 12, 19, 12);
        line(12, 5, 12, 19);
    } else if (name == s("back")) {
        line(5, 12, 20, 12);
        line(5, 12, 11, 6);
        line(5, 12, 11, 18);
    } else if (name == s("person")) {
        p.drawEllipse(QPointF(12, 8), 4, 4);
        p.drawArc(QRectF(4, 13, 16, 15), 0, 180 * 16);
    } else if (name == s("play")) {
        p.setBrush(color);
        p.setPen(Qt::NoPen);
        p.drawPolygon(QPolygonF(QVector<QPointF>{{7, 4}, {20, 12}, {7, 20}}));
    } else if (name == s("folder")) {
        p.drawPolygon(
            QPolygonF(QVector<QPointF>{{3, 5}, {10, 5}, {12, 8}, {21, 8}, {21, 20}, {3, 20}}));
    } else if (name == s("star")) {
        QPolygonF star;
        for (int i = 0; i < 10; ++i) {
            qreal a = (-90 + i * 36) * 3.141592653589793 / 180;
            qreal r = i % 2 ? 4 : 9;
            star << QPointF(12 + std::cos(a) * r, 12 + std::sin(a) * r);
        }
        p.drawPolygon(star);
    } else if (name == s("telegram")) {
        p.drawPolygon(
            QPolygonF(QVector<QPointF>{{3, 10}, {21, 3}, {17, 21}, {11, 16}, {8, 19}, {7, 13}}));
        line(7, 13, 17, 7);
        line(11, 16, 17, 7);
    } else if (name == s("discord")) {
        QPainterPath q;
        q.moveTo(7, 6);
        q.cubicTo(10, 5, 14, 5, 17, 6);
        q.cubicTo(20, 9, 21, 12, 21, 17);
        q.lineTo(16, 19);
        q.lineTo(15, 17);
        q.cubicTo(13, 18, 11, 18, 9, 17);
        q.lineTo(8, 19);
        q.lineTo(3, 17);
        q.cubicTo(3, 12, 4, 9, 7, 6);
        p.drawPath(q);
        p.drawEllipse(QPointF(9, 12), 1, 1.5);
        p.drawEllipse(QPointF(15, 12), 1, 1.5);
    } else if (name == s("github")) {
        p.drawEllipse(QRectF(3, 3, 18, 18));
        p.setBrush(color);
        p.drawEllipse(QRectF(7, 7, 10, 8));
        p.drawPolygon(
            QPolygonF(QVector<QPointF>{{7, 9}, {7, 5}, {10, 7}, {14, 7}, {17, 5}, {17, 9}}));
        p.drawRect(QRectF(10, 14, 4, 7));
    } else if (name == s("check")) {
        line(4, 12, 10, 18);
        line(10, 18, 21, 5);
    } else {
        p.setBrush(color);
        for (int x : {5, 12, 19})
            p.drawEllipse(QPointF(x, 12), 1, 1);
    }
    pix.setDevicePixelRatio(2);
    return QIcon(pix);
}
QPushButton *iconButton(const QString &name, const QString &text, QWidget *parent) {
    auto *b = new MotionButton(parent);
    b->setIcon(glyph(name));
    b->setIconSize(QSize(24, 24));
    b->setToolTip(text);
    b->setAccessibleName(text);
    b->setProperty("iconOnly", true);
    b->setFixedSize(42, 42);
    b->setCursor(Qt::PointingHandCursor);
    return b;
}
QFrame *panel(const QString &kind) {
    auto *w = new QFrame;
    w->setProperty("panel", true);
    if (!kind.isEmpty())
        w->setProperty("kind", kind);
    return w;
}
QLabel *label(const QString &text, const char *role) {
    auto *w = new QLabel(text);
    w->setTextFormat(Qt::PlainText);
    if (role)
        w->setProperty(role, true);
    if (role && (qstrcmp(role, "strong") == 0 || qstrcmp(role, "detailHeading") == 0 ||
                 qstrcmp(role, "chipTitle") == 0)) {
        w->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
        w->setToolTip(text);
    }
    return w;
}
void polish(QWidget *w) {
    w->style()->unpolish(w);
    w->style()->polish(w);
    w->update();
}
void clearLayout(QLayout *l) {
    while (auto *i = l->takeAt(0)) {
        if (i->widget()) {
            i->widget()->hide();
            i->widget()->deleteLater();
        }
        if (i->layout())
            clearLayout(i->layout());
        delete i;
    }
}
QWidget *scrollPage(QWidget *content) {
    auto *scroll = new QScrollArea;
    scroll->setWidgetResizable(true);
    scroll->setFrameShape(QFrame::NoFrame);
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    scroll->setWidget(content);
    return scroll;
}

Backdrop::Backdrop(QWidget *parent) : QWidget(parent) {
    for (int i = 1; i <= 3; ++i)
        scenes.append(QPixmap(s(":/assets/bg-%1.jpg").arg(i)));
    cycle.setInterval(14000);
    fade.setInterval(35);
    connect(&cycle, &QTimer::timeout, this, [this] {
        if (!isVisible() || window()->isMinimized())
            return;
        previous = current;
        current = (current + 1) % scenes.size();
        blend = 0;
        fade.start();
    });
    connect(&fade, &QTimer::timeout, this, [this] {
        blend = qMin(1.0, blend + 0.035);
        if (blend >= 1)
            fade.stop();
        update();
    });
    cycle.start();
}
void Backdrop::setMotion(bool enabled) {
    if (enabled)
        cycle.start();
    else {
        cycle.stop();
        fade.stop();
        blend = 1;
        update();
    }
}
void Backdrop::paintEvent(QPaintEvent *) {
    QPainter p(this);
    p.fillRect(rect(), QColor(5, 11, 21));
    auto draw = [&](const QPixmap &scene) {
        if (scene.isNull())
            return;
        QSizeF scaled = scene.size();
        scaled.scale(size(), Qt::KeepAspectRatioByExpanding);
        QRectF target((width() - scaled.width()) / 2, (height() - scaled.height()) / 2,
                      scaled.width(), scaled.height());
        p.drawPixmap(target, scene, scene.rect());
    };
    if (blend < 1)
        draw(scenes[previous]);
    p.setOpacity(blend);
    draw(scenes[current]);
    p.setOpacity(1);
    QLinearGradient shade(0, 0, 0, height());
    shade.setColorAt(0, QColor(3, 11, 22, 195));
    shade.setColorAt(.5, QColor(3, 10, 20, 200));
    shade.setColorAt(1, QColor(3, 9, 18, 240));
    p.fillRect(rect(), shade);
    QLinearGradient edge(0, 0, width(), 0);
    edge.setColorAt(0, QColor(4, 12, 23, 95));
    edge.setColorAt(.6, QColor(4, 12, 23, 0));
    edge.setColorAt(1, QColor(3, 10, 21, 100));
    p.fillRect(rect(), edge);
    p.setPen(QColor(110, 167, 220, 35));
    p.drawRect(rect().adjusted(0, 0, -1, -1));
}

CardGrid::CardGrid(int w, int h, int max, QWidget *parent)
    : QWidget(parent), grid(new QGridLayout(this)), minimumWidth(w), rowHeight(h), maxColumns(max) {
    grid->setContentsMargins(0, 0, 0, 0);
    grid->setSpacing(12);
    setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Fixed);
}
void CardGrid::append(QWidget *w) {
    w->setParent(this);
    w->setFixedHeight(rowHeight);
    w->setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Fixed);
    items.append(w);
    columns = 0;
    arrange();
}
void CardGrid::clear() {
    for (auto *w : items) {
        grid->removeWidget(w);
        w->hide();
        w->deleteLater();
    }
    items.clear();
    columns = 0;
    arrange();
}
void CardGrid::resizeEvent(QResizeEvent *e) {
    QWidget::resizeEvent(e);
    arrange();
}
void CardGrid::arrange() {
    int count = qBound(1, (width() + 12) / (minimumWidth + 12), maxColumns);
    if (count != columns) {
        for (auto *w : items)
            grid->removeWidget(w);
        for (int i = 0; i < items.size(); ++i)
            grid->addWidget(items[i], i / count, i % count);
        columns = count;
    }
    int rows = (items.size() + count - 1) / count;
    setFixedHeight(rows ? rows * rowHeight + (rows - 1) * 12 : 0);
}

ImagePool::ImagePool(Backend *core, QObject *parent)
    : QObject(parent), backend(core), cache(48 * 1024) {}
QImage ImagePool::decode(const QByteArray &bytes) {
    if (bytes.isEmpty() || bytes.size() > 2 * 1024 * 1024)
        return {};
    QBuffer buffer;
    buffer.setData(bytes);
    buffer.open(QIODevice::ReadOnly);
    QImageReader reader(&buffer);
    auto size = reader.size();
    auto format = reader.format();
    if (!size.isValid() || size.width() > 2048 || size.height() > 2048 ||
        !(format == "png" || format == "jpeg" || format == "webp" || format == "gif"))
        return {};
    return reader.read();
}
void ImagePool::load(const QString &url, QObject *owner, std::function<void(const QImage &)> done) {
    if (auto *image = cache.object(url)) {
        done(*image);
        return;
    }
    if (url.startsWith(s("data:image/png;base64,"))) {
        if (url.size() > 3 * 1024 * 1024) {
            done({});
            return;
        }
        auto image = decode(QByteArray::fromBase64(url.mid(22).toLatin1()));
        if (!image.isNull())
            cache.insert(url, new QImage(image), qMax(1, image.width() * image.height() / 256));
        done(image);
        return;
    }
    if (url.startsWith(s(":/assets/"))) {
        done(QImage(url));
        return;
    }
    if (url.isEmpty() || url.size() > 2048) {
        done({});
        return;
    }
    if (waiting.size() > 80 && !waiting.contains(url)) {
        done({});
        return;
    }
    waiting[url].append({owner, std::move(done)});
    if (waiting[url].size() == 1) {
        queue.enqueue(url);
        pump();
    }
}
QUrl ImagePool::remoteUrl(const QString &text) {
    const QStringList hosts{s("cdn.modrinth.com"), s("textures.minecraft.net"),
                            s("www.minecraft.net"), s("minecraft.net"), s("mc-heads.net")};
    QUrl url(text, QUrl::StrictMode);
    if (!url.isValid() || text.size() > 2048 || !hosts.contains(url.host().toLower()) ||
        !url.userInfo().isEmpty() || url.hasFragment())
        return {};
    if (url.scheme() == s("http") && (url.port() == -1 || url.port() == 80)) {
        url.setScheme(s("https"));
        url.setPort(-1);
    }
    if (url.scheme() != s("https") || (url.port() != -1 && url.port() != 443))
        return {};
    return url;
}
void ImagePool::pump() {
    while (activeRequests < 4 && !queue.isEmpty()) {
        const auto key = queue.dequeue();
        const auto url = remoteUrl(key);
        if (url.isEmpty()) {
            complete(key, {});
            continue;
        }
        ++activeRequests;
        QPointer<ImagePool> guard(this);
        backend->request(s("load_public_image"), {{s("url"), url.toString()}},
                         [guard, key](const QJsonValue &value, const QJsonObject &error) {
                             if (!guard)
                                 return;
                             --guard->activeRequests;
                             QImage image;
                             if (error.isEmpty() && value.isString() &&
                                 value.toString().size() <= 3 * 1024 * 1024)
                                 image =
                                     decode(QByteArray::fromBase64(value.toString().toLatin1()));
                             guard->complete(key, image);
                             if (guard)
                                 guard->pump();
                         });
    }
}
void ImagePool::complete(const QString &key, const QImage &image) {
    if (!image.isNull())
        cache.insert(key, new QImage(image), qMax(1, image.width() * image.height() / 256));
    auto listeners = waiting.take(key);
    for (const auto &w : listeners)
        if (w.owner)
            w.done(image);
}
Picture::Picture(int size, QWidget *parent) : QWidget(parent) {
    setFixedSize(size, size);
}
void Picture::setImage(const QImage &value) {
    image = value;
    update();
}
void Picture::setFallback(const QString &value) {
    fallback = value.left(2).toUpper();
    update();
}
void Picture::paintEvent(QPaintEvent *) {
    QPainter p(this);
    p.setRenderHint(QPainter::Antialiasing);
    QPainterPath clip;
    clip.addRoundedRect(rect(), width() * .19, width() * .19);
    p.setClipPath(clip);
    p.fillRect(rect(), QColor(22, 64, 94));
    if (!image.isNull()) {
        p.setRenderHint(QPainter::SmoothPixmapTransform, !pixelated);
        p.drawImage(rect(), image);
    } else {
        p.setPen(QColor(158, 220, 250));
        auto f = font();
        f.setPixelSize(width() / 3);
        f.setBold(true);
        p.setFont(f);
        p.drawText(rect(), Qt::AlignCenter, fallback);
    }
}
