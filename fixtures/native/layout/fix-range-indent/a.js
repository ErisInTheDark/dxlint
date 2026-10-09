require('http').request({hostname: 'localhost',
                  port: 80}, function(res) {
    res.end();
  });
