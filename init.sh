if [ "$1" == "" ];then
   echo arg1 empty
   exit 1
fi

CRATE_NAME=$1
sed -i.bak -r 's/(ARG CRATE_NAME=)discordbot_template/\1'$CRATE_NAME'/g' Dockerfile
sed -i.bak -r 's/(ARG CRATE_NAME=)discordbot_template/\1'$CRATE_NAME'/g' Dockerfile.musl
sed -i.bak -r 's/(name = )"discordbot_template"/\1"'$CRATE_NAME'"/g' Cargo.toml
sed -i.bak -r 's/(name = )"discordbot_template"/\1"'$CRATE_NAME'"/g' Cargo.lock
